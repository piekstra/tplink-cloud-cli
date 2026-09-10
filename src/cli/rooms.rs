//! `rooms list|devices|move|create|rename|delete` — the Tapo app's homes
//! and rooms, on the NBU app-server cloud (`api/nbu.rs`).
//!
//! Reads need nothing but the session. Writes confirm unless `--force`,
//! exit 6 non-interactively **before** any keychain or network work, and are
//! read back from the cloud before they are reported: the write endpoints
//! answer with empty bodies, so a 2xx alone proves nothing.

use std::future::Future;
use std::pin::Pin;

use clap::{Args, Subcommand};
use pk_cli_core::confirm::{confirm, require_confirmable};
use pk_cli_core::output::{emit_list, emit_one};
use pk_cli_core::resolve::pick;
use pk_cli_core::CliError;
use serde_json::{json, Value};

use super::emit::Ctx;
use super::groups::device_room_row;
use crate::api::client::TPLinkApi;
use crate::api::cloud_type::CloudType;
use crate::api::nbu::{self, Family, NbuClient, Room, Thing};
use crate::error::AppError;
use crate::resolve;
use crate::session::{self, TokenSet};

/// How long a resolved app-server host is trusted (what the app uses).
const APP_SERVER_TTL_SECS: u64 = 24 * 60 * 60;

#[derive(Args, Debug, Clone)]
pub struct HomeArg {
    /// Home (family) id or name; defaults to the only/default home.
    #[arg(long, value_name = "ID|NAME")]
    pub home: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum RoomsCommand {
    /// Rooms with their device counts (room-list/v1).
    #[command(visible_alias = "ls")]
    List(HomeArg),
    /// Every device that is in a room, as `device-rooms/v1` for
    /// `ghome audit --expect -` (pass --json when piping).
    Devices(HomeArg),
    /// Put a device in a room (prompts; --force to skip).
    Move {
        /// Device name or ID
        device: String,
        /// Target room id or name
        #[arg(long, value_name = "ID|NAME")]
        room: String,
        #[command(flatten)]
        home: HomeArg,
        /// Skip the confirmation prompt (required non-interactively).
        #[arg(long)]
        force: bool,
    },
    /// Create a room (prompts; --force to skip).
    Create {
        /// Room name
        name: String,
        #[command(flatten)]
        home: HomeArg,
        #[arg(long)]
        force: bool,
    },
    /// Rename a room (prompts; --force to skip).
    Rename {
        /// Room id or current name
        room: String,
        /// New name
        name: String,
        #[command(flatten)]
        home: HomeArg,
        #[arg(long)]
        force: bool,
    },
    /// Delete an empty room (prompts; --force to skip).
    Delete {
        /// Room id or name
        room: String,
        #[command(flatten)]
        home: HomeArg,
        #[arg(long)]
        force: bool,
    },
}

/// The mutation gate, evaluated before anything else so a non-interactive
/// run without `--force` is exit 6 with no keychain read (SPEC §1.3).
pub fn gate(cmd: &RoomsCommand, interactive: bool) -> Result<(), CliError> {
    match cmd {
        RoomsCommand::Move {
            device,
            room,
            force,
            ..
        } => require_confirmable(*force, interactive, &format!("move `{device}` to `{room}`")),
        RoomsCommand::Create { name, force, .. } => {
            require_confirmable(*force, interactive, &format!("create room `{name}`"))
        }
        RoomsCommand::Rename {
            room, name, force, ..
        } => require_confirmable(
            *force,
            interactive,
            &format!("rename room `{room}` to `{name}`"),
        ),
        RoomsCommand::Delete { room, force, .. } => {
            require_confirmable(*force, interactive, &format!("delete room `{room}`"))
        }
        RoomsCommand::List(_) | RoomsCommand::Devices(_) => Ok(()),
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A connected Tapo rooms session: the tokens, the NBU client, and one
/// refresh-and-retry on an expired Tapo token.
struct Tapo<'a> {
    ctx: &'a Ctx<'a>,
    tokens: TokenSet,
    client: NbuClient,
}

impl<'a> Tapo<'a> {
    async fn connect(ctx: &'a Ctx<'a>) -> Result<Self, CliError> {
        let mut tokens = ctx.session()?;
        if !tokens.has_tapo() {
            return Err(CliError::Auth(
                "this session has no Tapo login (rooms live in the Tapo app); run `tplc auth login` again".into(),
            ));
        }
        let base = app_server_url(ctx, &mut tokens).await?;
        let (token, _) = tokens.cloud_access(CloudType::Tapo)?;
        let client = NbuClient::new(&base, &token, &tokens.term_id, ctx.verbose)?;
        Ok(Tapo {
            ctx,
            tokens,
            client,
        })
    }

    async fn refresh(&mut self) -> Result<(), CliError> {
        session::refresh(
            self.ctx.sessions,
            &mut self.tokens,
            CloudType::Tapo,
            self.ctx.verbose,
        )
        .await?;
        let (token, _) = self.tokens.cloud_access(CloudType::Tapo)?;
        self.client.set_token(&token);
        Ok(())
    }

    /// Run one NBU call; on an expired token, refresh once and run it again.
    async fn call<T>(
        &mut self,
        op: impl for<'c> Fn(&'c NbuClient) -> Pin<Box<dyn Future<Output = Result<T, AppError>> + 'c>>,
    ) -> Result<T, CliError> {
        match op(&self.client).await {
            Err(AppError::TokenExpired { .. }) => {
                self.refresh().await?;
                op(&self.client).await.map_err(Into::into)
            }
            other => other.map_err(Into::into),
        }
    }

    async fn families(&mut self) -> Result<Vec<Family>, CliError> {
        self.call(|c| Box::pin(c.families())).await
    }

    async fn things(&mut self) -> Result<Vec<Thing>, CliError> {
        self.call(|c| Box::pin(c.things())).await
    }
}

/// The NBU app-server host for this account, from the session cache or a
/// fresh `getAppServiceUrl` (persisted with a 24h expiry).
async fn app_server_url(ctx: &Ctx<'_>, tokens: &mut TokenSet) -> Result<String, CliError> {
    if let (Some(url), Some(exp)) = (
        &tokens.tapo_app_server_url,
        tokens.tapo_app_server_expires_at,
    ) {
        if now_unix() < exp {
            return Ok(url.clone());
        }
    }
    let url = match resolve_app_server(ctx, tokens).await {
        Err(AppError::TokenExpired { .. }) => {
            session::refresh(ctx.sessions, tokens, CloudType::Tapo, ctx.verbose).await?;
            resolve_app_server(ctx, tokens).await?
        }
        other => other?,
    };
    tokens.tapo_app_server_url = Some(url.clone());
    tokens.tapo_app_server_expires_at = Some(now_unix() + APP_SERVER_TTL_SECS);
    // A failed cache write is not worth failing the command over.
    let _ = ctx.sessions.store(tokens);
    Ok(url)
}

async fn resolve_app_server(ctx: &Ctx<'_>, tokens: &TokenSet) -> Result<String, AppError> {
    let (token, regional_url) = tokens.cloud_access(CloudType::Tapo)?;
    let api = TPLinkApi::new(
        Some(regional_url),
        ctx.verbose,
        Some(tokens.term_id.clone()),
        CloudType::Tapo,
    )?;
    api.get_app_service_url(&token, nbu::APP_SERVER_SERVICE_ID)
        .await
}

/// A room with the home it belongs to, for resolution and rendering.
#[derive(Debug, Clone)]
struct RoomRef<'f> {
    home: &'f Family,
    room: &'f Room,
}

fn rooms_of<'f>(homes: &[&'f Family]) -> Vec<RoomRef<'f>> {
    homes
        .iter()
        .flat_map(|h| h.rooms.iter().map(move |r| RoomRef { home: h, room: r }))
        .collect()
}

/// Homes to act on: `--home` if given, else all of them.
fn select_homes<'f>(
    families: &'f [Family],
    flag: Option<&str>,
) -> Result<Vec<&'f Family>, CliError> {
    match flag {
        Some(q) => Ok(vec![pick(
            families,
            q,
            |f| vec![f.id.clone()],
            |f| &f.name,
            "home",
        )?]),
        None => Ok(families.iter().collect()),
    }
}

/// The one home a write goes to when `--home` is absent: the only home, or
/// the default one; several without a default is a usage error.
fn default_home(families: &[Family]) -> Result<&Family, CliError> {
    match families {
        [] => Err(CliError::NotFound("no home on this Tapo account".into())),
        [one] => Ok(one),
        many => many.iter().find(|f| f.is_default).ok_or_else(|| {
            CliError::Usage(format!(
                "several homes and no default; pass --home ({})",
                many.iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }),
    }
}

fn find_room<'f>(homes: &[&'f Family], q: &str) -> Result<RoomRef<'f>, CliError> {
    let rooms = rooms_of(homes);
    let hit = pick(
        &rooms,
        q,
        |r| vec![r.room.id.clone()],
        |r| &r.room.name,
        "room",
    )?;
    Ok(hit.clone())
}

/// Resolve a device among the Tapo cloud's things by id or (decoded) name.
fn find_thing<'t>(things: &'t [Thing], q: &str) -> Result<&'t Thing, CliError> {
    let named: Vec<(String, &Thing)> = things
        .iter()
        .map(|t| (t.display_name().unwrap_or_default(), t))
        .collect();
    let hit = pick(
        &named,
        q,
        |x| vec![x.1.thing_name.clone()],
        |x| &x.0,
        "device",
    )?;
    Ok(hit.1)
}

fn room_dto(r: &RoomRef, devices: usize) -> Value {
    json!({
        "name": r.room.name,
        "devices": devices,
        "home": r.home.name,
        "id": r.room.id,
        "home_id": r.home.id,
    })
}

fn count_in(things: &[Thing], room_id: &str) -> usize {
    things
        .iter()
        .filter(|t| t.room_id.as_deref() == Some(room_id))
        .count()
}

pub async fn handle(ctx: &Ctx<'_>, cmd: &RoomsCommand) -> Result<(), CliError> {
    gate(cmd, ctx.interactive)?;
    let mut tapo = Tapo::connect(ctx).await?;
    match cmd {
        RoomsCommand::List(HomeArg { home }) => {
            let families = tapo.families().await?;
            let things = tapo.things().await?;
            let homes = select_homes(&families, home.as_deref())?;
            let items = rooms_of(&homes)
                .iter()
                .map(|r| room_dto(r, count_in(&things, &r.room.id)))
                .collect();
            emit_list(ctx.json, "room", items, &["name", "devices", "home", "id"]);
            Ok(())
        }
        RoomsCommand::Devices(HomeArg { home }) => {
            let families = tapo.families().await?;
            let things = tapo.things().await?;
            let homes = select_homes(&families, home.as_deref())?;
            let rooms = rooms_of(&homes);
            // Names come from the account cloud's device list (the alias the
            // user knows), falling back to the Tapo nickname.
            let (devices, _) = resolve::fetch_all_devices_with(ctx, false).await?;
            let alias_of = |id: &str| {
                devices
                    .iter()
                    .find(|d| d.child_id.is_none() && d.info.id() == id)
                    .map(|d| d.name().to_string())
            };
            let items = things
                .iter()
                .filter_map(|t| {
                    let room_id = t.room_id.as_deref()?;
                    let room = rooms.iter().find(|r| r.room.id == room_id)?;
                    let name = alias_of(&t.thing_name).or_else(|| t.display_name());
                    Some(device_room_row(&t.thing_name, name, &room.room.name))
                })
                .collect();
            emit_list(ctx.json, "device-rooms", items, &["name", "room", "id"]);
            Ok(())
        }
        RoomsCommand::Move {
            device,
            room,
            home,
            force,
        } => {
            let families = tapo.families().await?;
            let things = tapo.things().await?;
            let thing = find_thing(&things, device)?;
            let name = thing
                .display_name()
                .unwrap_or_else(|| thing.thing_name.clone());
            // The target room must be in the device's own home unless told otherwise.
            let homes = match home.home.as_deref() {
                Some(h) => select_homes(&families, Some(h))?,
                None => match &thing.family_id {
                    Some(fid) => families.iter().filter(|f| &f.id == fid).collect(),
                    None => vec![default_home(&families)?],
                },
            };
            let target = find_room(&homes, room)?;
            let previous = thing
                .room_id
                .as_deref()
                .and_then(|rid| {
                    rooms_of(&families.iter().collect::<Vec<_>>())
                        .into_iter()
                        .find(|r| r.room.id == rid)
                })
                .map(|r| r.room.name.clone());
            if thing.room_id.as_deref() == Some(target.room.id.as_str()) {
                emit_one(
                    ctx.json,
                    "room-move",
                    json!({
                        "device": name, "device_id": thing.thing_name,
                        "room": target.room.name, "room_id": target.room.id,
                        "home": target.home.name, "home_id": target.home.id,
                        "changed": false,
                    }),
                );
                return Ok(());
            }
            confirm(
                *force,
                &format!(
                    "Move `{name}` to `{}` in `{}`?",
                    target.room.name, target.home.name
                ),
            )?;
            let (fid, rid, tn) = (
                target.home.id.clone(),
                target.room.id.clone(),
                vec![thing.thing_name.clone()],
            );
            tapo.call(|c| Box::pin(c.move_things(fid.clone(), rid.clone(), tn.clone())))
                .await?;
            // Read back: the write answers with an empty body.
            let after = tapo.things().await?;
            let placed = after
                .iter()
                .find(|t| t.thing_name == tn[0])
                .is_some_and(|t| t.room_id.as_deref() == Some(rid.as_str()));
            if !placed {
                return Err(CliError::Upstream(format!(
                    "the Tapo cloud accepted the move but `{name}` is not in `{}` on read-back",
                    target.room.name
                )));
            }
            let mut dto = json!({
                "device": name, "device_id": tn[0],
                "room": target.room.name, "room_id": rid,
                "home": target.home.name, "home_id": fid,
                "changed": true,
            });
            if let Some(p) = previous {
                dto["previous_room"] = json!(p);
            }
            emit_one(ctx.json, "room-move", dto);
            Ok(())
        }
        RoomsCommand::Create { name, home, force } => {
            let families = tapo.families().await?;
            let target = match home.home.as_deref() {
                Some(h) => select_homes(&families, Some(h))?[0],
                None => default_home(&families)?,
            };
            if let Some(existing) = target
                .rooms
                .iter()
                .find(|r| r.name.eq_ignore_ascii_case(name))
            {
                return Err(CliError::Usage(format!(
                    "`{}` already has a room named `{}` ({})",
                    target.name, existing.name, existing.id
                )));
            }
            confirm(
                *force,
                &format!("Create room `{name}` in `{}`?", target.name),
            )?;
            let (fid, rid, rname) = (target.id.clone(), nbu::new_room_id(), name.clone());
            tapo.call(|c| Box::pin(c.upsert_room(fid.clone(), rid.clone(), rname.clone())))
                .await?;
            let after = tapo.families().await?;
            let created = after
                .iter()
                .find(|f| f.id == fid)
                .and_then(|f| f.rooms.iter().find(|r| r.id == rid))
                .cloned()
                .ok_or_else(|| {
                    CliError::Upstream(format!(
                        "the Tapo cloud accepted the room but `{rname}` is missing on read-back"
                    ))
                })?;
            emit_one(
                ctx.json,
                "room",
                json!({"id": created.id, "name": created.name, "home": target.name, "home_id": fid, "devices": 0}),
            );
            Ok(())
        }
        RoomsCommand::Rename {
            room,
            name,
            home,
            force,
        } => {
            let families = tapo.families().await?;
            let homes = select_homes(&families, home.home.as_deref())?;
            let target = find_room(&homes, room)?;
            confirm(
                *force,
                &format!(
                    "Rename `{}` to `{name}` in `{}`?",
                    target.room.name, target.home.name
                ),
            )?;
            let (fid, rid, rname) = (target.home.id.clone(), target.room.id.clone(), name.clone());
            tapo.call(|c| Box::pin(c.upsert_room(fid.clone(), rid.clone(), rname.clone())))
                .await?;
            let after = tapo.families().await?;
            let renamed = after
                .iter()
                .find(|f| f.id == fid)
                .and_then(|f| f.rooms.iter().find(|r| r.id == rid))
                .filter(|r| r.name == rname)
                .cloned()
                .ok_or_else(|| {
                    CliError::Upstream(format!(
                        "the Tapo cloud accepted the rename but `{rid}` is not `{rname}` on read-back"
                    ))
                })?;
            let things = tapo.things().await?;
            emit_one(
                ctx.json,
                "room",
                json!({
                    "id": renamed.id, "name": renamed.name,
                    "home": target.home.name, "home_id": fid,
                    "devices": count_in(&things, &rid),
                    "previous_name": target.room.name,
                }),
            );
            Ok(())
        }
        RoomsCommand::Delete { room, home, force } => {
            let families = tapo.families().await?;
            let things = tapo.things().await?;
            let homes = select_homes(&families, home.home.as_deref())?;
            let target = find_room(&homes, room)?;
            let members = count_in(&things, &target.room.id);
            if members > 0 {
                return Err(CliError::Usage(format!(
                    "room `{}` still has {members} device(s); move them out first (`tplc rooms move`)",
                    target.room.name
                )));
            }
            confirm(
                *force,
                &format!(
                    "Delete room `{}` in `{}`?",
                    target.room.name, target.home.name
                ),
            )?;
            let (fid, rid) = (target.home.id.clone(), target.room.id.clone());
            tapo.call(|c| Box::pin(c.delete_room(fid.clone(), rid.clone())))
                .await?;
            let after = tapo.families().await?;
            let still_there = after
                .iter()
                .find(|f| f.id == fid)
                .is_some_and(|f| f.rooms.iter().any(|r| r.id == rid));
            if still_there {
                return Err(CliError::Upstream(format!(
                    "the Tapo cloud accepted the delete but `{}` is still listed on read-back",
                    target.room.name
                )));
            }
            emit_one(
                ctx.json,
                "room-delete",
                json!({"id": rid, "name": target.room.name, "home": target.home.name, "home_id": fid, "deleted": true}),
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn families() -> Vec<Family> {
        serde_json::from_value(json!([
            {"id": "FAM00001", "name": "Home", "default": true,
             "rooms": [{"id": "ROOM0001", "name": "Office"}, {"id": "ROOM0002", "name": "Kitchen"}]},
            {"id": "FAM00002", "name": "Cabin", "default": false,
             "rooms": [{"id": "ROOM0003", "name": "Office"}]}
        ]))
        .unwrap()
    }

    #[test]
    fn home_selection() {
        let f = families();
        assert_eq!(select_homes(&f, None).unwrap().len(), 2);
        assert_eq!(select_homes(&f, Some("cabin")).unwrap()[0].id, "FAM00002");
        assert_eq!(select_homes(&f, Some("FAM00001")).unwrap()[0].name, "Home");
        assert_eq!(default_home(&f).unwrap().id, "FAM00001");
        assert!(select_homes(&f, Some("garage")).is_err());
    }

    #[test]
    fn room_lookup_is_scoped_by_home_and_names_ambiguity() {
        let f = families();
        let all: Vec<&Family> = f.iter().collect();
        let err = find_room(&all, "office").unwrap_err();
        assert!(err.to_string().contains("more than one room"), "{err}");
        let one = select_homes(&f, Some("Cabin")).unwrap();
        assert_eq!(find_room(&one, "office").unwrap().room.id, "ROOM0003");
        assert_eq!(find_room(&all, "kitch").unwrap().room.id, "ROOM0002");
    }

    #[test]
    fn things_resolve_by_id_or_decoded_nickname() {
        let things: Vec<Thing> = serde_json::from_value(json!([
            {"thingName": "0000000000000000000000000000000000000001", "nickname": "T2ZmaWNlIExhbXA="},
            {"thingName": "0000000000000000000000000000000000000002", "nickname": "Kitchen Plug"}
        ]))
        .unwrap();
        assert_eq!(
            find_thing(&things, "office lamp").unwrap().thing_name,
            "0000000000000000000000000000000000000001"
        );
        assert_eq!(
            find_thing(&things, "0000000000000000000000000000000000000002")
                .unwrap()
                .nickname
                .as_deref(),
            Some("Kitchen Plug")
        );
        assert!(find_thing(&things, "garage").is_err());
    }

    #[test]
    fn writes_are_gated_before_any_io() {
        let mv = RoomsCommand::Move {
            device: "Lamp".into(),
            room: "Office".into(),
            home: HomeArg { home: None },
            force: false,
        };
        let e = gate(&mv, false).unwrap_err();
        assert_eq!(e.exit_code(), 6);
        assert!(gate(&mv, true).is_ok(), "interactive runs prompt instead");
        let forced = RoomsCommand::Delete {
            room: "Office".into(),
            home: HomeArg { home: None },
            force: true,
        };
        assert!(gate(&forced, false).is_ok());
        assert!(gate(&RoomsCommand::List(HomeArg { home: None }), false).is_ok());
    }
}
