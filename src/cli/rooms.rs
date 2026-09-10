//! `rooms list|devices|move|create|rename|delete` — the Tapo app's homes
//! and rooms, on the NBU app-server cloud (`api/nbu.rs`).
//!
//! Reads need nothing but the session. Writes confirm unless `--force`,
//! exit 6 non-interactively **before** any keychain or network work, and are
//! read back from the cloud before they are reported: the write endpoints
//! answer with empty bodies, so a 2xx alone proves nothing.
//!
//! `handle` is fetch → rule → write → verify. The rules (idempotence,
//! refusals, the read-back predicates) are the pure functions below, tested
//! against the fixture pages in `tests/fixtures/nbu_*.json`.

use std::future::Future;

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
    /// Every device with its room, as `device-rooms/v1` for
    /// `ghome audit --expect -` (pass --json when piping). A Tapo device in
    /// no room keeps its row without `room`.
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

// ---- the NBU connection ----------------------------------------------------

/// A connected Tapo rooms session: the tokens and the NBU client. Every
/// call goes through [`session::with_refresh`], so an expired Tapo token is
/// refreshed once and the call retried with the new token.
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

    /// Run one NBU call with the current Tapo token, refreshing once on
    /// expiry. `op` receives a client presenting whichever token is current.
    async fn call<T, F, Fut>(&mut self, op: F) -> Result<T, CliError>
    where
        F: Fn(NbuClient) -> Fut,
        Fut: Future<Output = Result<T, AppError>>,
    {
        let client = &self.client;
        session::with_refresh(
            self.ctx.sessions,
            &mut self.tokens,
            CloudType::Tapo,
            self.ctx.verbose,
            |t| op(client.with_token(t.tapo_token.as_deref().unwrap_or_default())),
        )
        .await
    }

    async fn families(&mut self) -> Result<Vec<Family>, CliError> {
        self.call(|c| async move { c.families().await }).await
    }

    async fn things(&mut self) -> Result<Vec<Thing>, CliError> {
        self.call(|c| async move { c.things().await }).await
    }
}

/// The cached app-server host, if the session has one that has not
/// expired at `now` (Unix seconds).
pub fn cached_app_server(tokens: &TokenSet, now: u64) -> Option<String> {
    match (
        &tokens.tapo_app_server_url,
        tokens.tapo_app_server_expires_at,
    ) {
        (Some(url), Some(exp)) if now < exp => Some(url.clone()),
        _ => None,
    }
}

/// The NBU app-server host for this account, from the session cache or a
/// fresh `getAppServiceUrl` (persisted with a 24h expiry).
async fn app_server_url(ctx: &Ctx<'_>, tokens: &mut TokenSet) -> Result<String, CliError> {
    let now = now_unix();
    if let Some(url) = cached_app_server(tokens, now) {
        return Ok(url);
    }
    let verbose = ctx.verbose;
    let url = session::with_refresh(ctx.sessions, tokens, CloudType::Tapo, verbose, |t| {
        let access = t.cloud_access(CloudType::Tapo);
        let term_id = t.term_id.clone();
        async move {
            let (token, regional_url) = access?;
            let api = TPLinkApi::new(Some(regional_url), verbose, Some(term_id), CloudType::Tapo)?;
            api.get_app_service_url(&token, nbu::APP_SERVER_SERVICE_ID)
                .await
        }
    })
    .await?;
    tokens.tapo_app_server_url = Some(url.clone());
    tokens.tapo_app_server_expires_at = Some(now + APP_SERVER_TTL_SECS);
    // A failed cache write is not worth failing the command over.
    let _ = ctx.sessions.store(tokens);
    Ok(url)
}

// ---- resolution ------------------------------------------------------------

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

/// The home a thing that names none belongs to, among `homes`: the only
/// one (flagged or not), else the one flagged default. The single
/// definition behind `default_home` (writes) and `device_room_rows`.
fn default_of<'f>(homes: &[&'f Family]) -> Option<&'f Family> {
    match homes {
        [one] => Some(one),
        many => many.iter().copied().find(|f| f.is_default),
    }
}

/// The one home a write goes to when `--home` is absent: `default_of`,
/// with several homes and no default a usage error.
fn default_home(families: &[Family]) -> Result<&Family, CliError> {
    if families.is_empty() {
        return Err(CliError::NotFound("no home on this Tapo account".into()));
    }
    let all: Vec<&Family> = families.iter().collect();
    default_of(&all).ok_or_else(|| {
        CliError::Usage(format!(
            "several homes and no default; pass --home ({})",
            families
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
}

/// Exactly one home: the `--home` reference when given, else the default.
pub fn one_home<'f>(families: &'f [Family], flag: Option<&str>) -> Result<&'f Family, CliError> {
    match flag {
        Some(q) => pick(families, q, |f| vec![f.id.clone()], |f| &f.name, "home"),
        None => default_home(families),
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

/// The homes a move may target: `--home` if given, else the device's own
/// home, else the default home.
fn target_homes<'f>(
    families: &'f [Family],
    thing: &Thing,
    flag: Option<&str>,
) -> Result<Vec<&'f Family>, CliError> {
    match (flag, &thing.family_id) {
        (Some(h), _) => select_homes(families, Some(h)),
        (None, Some(fid)) => Ok(families.iter().filter(|f| &f.id == fid).collect()),
        (None, None) => Ok(vec![default_home(families)?]),
    }
}

// ---- the rules -------------------------------------------------------------

/// A move to the room the device is already in is a no-op, not a write.
pub fn already_in_room(thing: &Thing, room_id: &str) -> bool {
    thing.room_id.as_deref() == Some(room_id)
}

/// The room a device is in before a move, if it is in one.
pub fn previous_room<'f>(families: &'f [Family], thing: &Thing) -> Option<&'f Room> {
    let rid = thing.room_id.as_deref()?;
    families
        .iter()
        .flat_map(|f| f.rooms.iter())
        .find(|r| r.id == rid)
}

/// `create` refuses a name the home already uses (case-insensitively): the
/// endpoint is an upsert on id and would happily make a second "Office".
pub fn refuse_duplicate(home: &Family, name: &str) -> Result<(), CliError> {
    match home
        .rooms
        .iter()
        .find(|r| r.name.eq_ignore_ascii_case(name))
    {
        Some(existing) => Err(CliError::Usage(format!(
            "`{}` already has a room named `{}` ({})",
            home.name, existing.name, existing.id
        ))),
        None => Ok(()),
    }
}

/// `delete` refuses a room that still holds devices: the cloud would strand
/// them, and the user meant to move them.
pub fn refuse_non_empty(things: &[Thing], room: &Room) -> Result<(), CliError> {
    let members = count_in(things, &room.id);
    if members > 0 {
        return Err(CliError::Usage(format!(
            "room `{}` still has {members} device(s); move them out first (`tplc rooms move`)",
            room.name
        )));
    }
    Ok(())
}

/// Read-back for a move: the device now reports the target room.
pub fn placed_in(things: &[Thing], thing_name: &str, room_id: &str) -> bool {
    things
        .iter()
        .find(|t| t.thing_name == thing_name)
        .is_some_and(|t| already_in_room(t, room_id))
}

/// Read-back for create/rename: the room exists with the expected name.
pub fn room_named<'f>(families: &'f [Family], room_id: &str, name: &str) -> Option<&'f Room> {
    families
        .iter()
        .flat_map(|f| f.rooms.iter())
        .find(|r| r.id == room_id && r.name == name)
}

/// Read-back for delete: no home lists the room any more.
pub fn room_gone(families: &[Family], room_id: &str) -> bool {
    !families
        .iter()
        .flat_map(|f| f.rooms.iter())
        .any(|r| r.id == room_id)
}

/// The `device-rooms/v1` rows for `homes`: one per thing filed in one of
/// their rooms, with the id Google Home joins on (`Thing::google_id`) and
/// the account cloud's alias over the Tapo nickname. One of Tapo's own
/// devices in no room keeps a row without `room`, so a consumer can report
/// the gap; a Kasa device shared into the Tapo app is the Kasa app's to
/// file (`groups devices`), so roomless it is left out. A thing in no home
/// counts as the default home's among `homes` (`default_of`, the rule
/// `rooms move` uses).
pub fn device_room_rows(
    things: &[Thing],
    homes: &[&Family],
    alias_of: impl Fn(&str) -> Option<String>,
) -> Vec<Value> {
    let rooms = rooms_of(homes);
    let home_less = default_of(homes);
    things
        .iter()
        .filter_map(|t| {
            let room = match t.room_id.as_deref() {
                Some(rid) => Some(rooms.iter().find(|r| r.room.id == rid)?),
                None => None,
            };
            let in_home = match t.family_id.as_deref() {
                Some(fid) => homes.iter().any(|h| h.id == fid),
                None => home_less.is_some(),
            };
            let in_scope = room.is_some() || (t.is_native_tapo() && in_home);
            if !in_scope {
                return None;
            }
            let name = alias_of(&t.thing_name).or_else(|| t.display_name());
            Some(device_room_row(
                &t.google_id(),
                name,
                room.map(|r| r.room.name.as_str()),
            ))
        })
        .collect()
}

fn count_in(things: &[Thing], room_id: &str) -> usize {
    things
        .iter()
        .filter(|t| already_in_room(t, room_id))
        .count()
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

fn not_reflected(what: &str) -> CliError {
    CliError::Upstream(format!(
        "the Tapo cloud accepted the write but {what} on read-back"
    ))
}

// ---- the commands ----------------------------------------------------------

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
            // Names come from the account cloud's device list (the alias the
            // user knows, decoded at that boundary), falling back to the
            // Tapo nickname.
            let (devices, _) = resolve::fetch_all_devices_with(ctx, false).await?;
            let alias_of = |id: &str| {
                devices
                    .iter()
                    .find(|d| d.child_id.is_none() && d.info.id() == id)
                    .map(|d| d.name().to_string())
            };
            let items = device_room_rows(&things, &homes, alias_of);
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
            let homes = target_homes(&families, thing, home.home.as_deref())?;
            let target = find_room(&homes, room)?;
            let mut dto = json!({
                "device": name, "device_id": thing.thing_name,
                "room": target.room.name, "room_id": target.room.id,
                "home": target.home.name, "home_id": target.home.id,
            });
            if already_in_room(thing, &target.room.id) {
                dto["changed"] = json!(false);
                emit_one(ctx.json, "room-move", dto);
                return Ok(());
            }
            if let Some(prev) = previous_room(&families, thing) {
                dto["previous_room"] = json!(prev.name);
            }
            confirm(
                *force,
                &format!(
                    "Move `{name}` to `{}` in `{}`?",
                    target.room.name, target.home.name
                ),
            )?;
            let (fid, rid) = (target.home.id.as_str(), target.room.id.as_str());
            let names = vec![thing.thing_name.clone()];
            let names = &names;
            tapo.call(|c| async move { c.move_things(fid, rid, names).await })
                .await?;
            let after = tapo.things().await?;
            if !placed_in(&after, &names[0], rid) {
                return Err(not_reflected(&format!(
                    "`{name}` is not in `{}`",
                    target.room.name
                )));
            }
            dto["changed"] = json!(true);
            emit_one(ctx.json, "room-move", dto);
            Ok(())
        }
        RoomsCommand::Create { name, home, force } => {
            let families = tapo.families().await?;
            let target = one_home(&families, home.home.as_deref())?;
            refuse_duplicate(target, name)?;
            confirm(
                *force,
                &format!("Create room `{name}` in `{}`?", target.name),
            )?;
            let new_id = nbu::new_room_id();
            let (fid, rid) = (target.id.as_str(), new_id.as_str());
            tapo.call(|c| async move { c.upsert_room(fid, rid, name).await })
                .await?;
            let after = tapo.families().await?;
            let created = room_named(&after, rid, name)
                .ok_or_else(|| not_reflected(&format!("room `{name}` is missing")))?;
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
            let (fid, rid) = (target.home.id.as_str(), target.room.id.as_str());
            tapo.call(|c| async move { c.upsert_room(fid, rid, name).await })
                .await?;
            let after = tapo.families().await?;
            let renamed = room_named(&after, rid, name)
                .ok_or_else(|| not_reflected(&format!("room `{rid}` is not named `{name}`")))?;
            let things = tapo.things().await?;
            emit_one(
                ctx.json,
                "room",
                json!({
                    "id": renamed.id, "name": renamed.name,
                    "home": target.home.name, "home_id": fid,
                    "devices": count_in(&things, rid),
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
            refuse_non_empty(&things, target.room)?;
            confirm(
                *force,
                &format!(
                    "Delete room `{}` in `{}`?",
                    target.room.name, target.home.name
                ),
            )?;
            let (fid, rid) = (target.home.id.as_str(), target.room.id.as_str());
            tapo.call(|c| async move { c.delete_room(fid, rid).await })
                .await?;
            let after = tapo.families().await?;
            if !room_gone(&after, rid) {
                return Err(not_reflected(&format!(
                    "`{}` is still listed",
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

    /// The fixture pages (`tests/fixtures/README.md`): one default home with
    /// three rooms; three things, two in rooms, one unassigned.
    fn fixture_families() -> Vec<Family> {
        let page: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/nbu_families_response.json"
        ))
        .unwrap();
        serde_json::from_value(page["data"].clone()).unwrap()
    }

    fn fixture_things() -> Vec<Thing> {
        let page: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/nbu_things_response.json"
        ))
        .unwrap();
        serde_json::from_value(page["data"].clone()).unwrap()
    }

    fn two_homes() -> Vec<Family> {
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
        let f = two_homes();
        assert_eq!(select_homes(&f, None).unwrap().len(), 2);
        assert_eq!(select_homes(&f, Some("cabin")).unwrap()[0].id, "FAM00002");
        assert_eq!(select_homes(&f, Some("FAM00001")).unwrap()[0].name, "Home");
        assert_eq!(default_home(&f).unwrap().id, "FAM00001");
        assert!(select_homes(&f, Some("garage")).is_err());
        // one_home: the reference when given, the default otherwise.
        assert_eq!(one_home(&f, Some("cabin")).unwrap().id, "FAM00002");
        assert_eq!(one_home(&f, None).unwrap().id, "FAM00001");
        assert!(one_home(&f, Some("garage")).is_err());
        let no_default: Vec<Family> = f
            .into_iter()
            .map(|mut h| {
                h.is_default = false;
                h
            })
            .collect();
        assert_eq!(one_home(&no_default, None).unwrap_err().exit_code(), 2);
    }

    #[test]
    fn room_lookup_is_scoped_by_home_and_names_ambiguity() {
        let f = two_homes();
        let all: Vec<&Family> = f.iter().collect();
        let err = find_room(&all, "office").unwrap_err();
        assert!(err.to_string().contains("more than one room"), "{err}");
        let one = select_homes(&f, Some("Cabin")).unwrap();
        assert_eq!(find_room(&one, "office").unwrap().room.id, "ROOM0003");
        assert_eq!(find_room(&all, "kitch").unwrap().room.id, "ROOM0002");
    }

    #[test]
    fn things_resolve_by_id_or_decoded_nickname() {
        let things = fixture_things();
        assert_eq!(
            find_thing(&things, "bedroom desk light")
                .unwrap()
                .thing_name,
            "0000000000000000000000000000000000000001"
        );
        assert_eq!(
            find_thing(&things, "TAPO_P100_ABCDEF1234567890ABCDEF1234567890")
                .unwrap()
                .display_name()
                .as_deref(),
            Some("Kitchen Tapo Plug")
        );
        assert!(find_thing(&things, "garage").is_err());
    }

    #[test]
    fn a_move_targets_the_devices_own_home_unless_told_otherwise() {
        let f = two_homes();
        let in_cabin: Thing = serde_json::from_value(json!({
            "thingName": "0000000000000000000000000000000000000009", "familyId": "FAM00002", "roomId": "ROOM0003"
        }))
        .unwrap();
        let homes = target_homes(&f, &in_cabin, None).unwrap();
        assert_eq!(homes.len(), 1);
        assert_eq!(homes[0].id, "FAM00002");
        assert_eq!(
            target_homes(&f, &in_cabin, Some("Home")).unwrap()[0].id,
            "FAM00001"
        );
        let homeless: Thing = serde_json::from_value(json!({"thingName": "x"})).unwrap();
        assert_eq!(target_homes(&f, &homeless, None).unwrap()[0].id, "FAM00001");
    }

    #[test]
    fn move_idempotence_and_previous_room() {
        let families = fixture_families();
        let things = fixture_things();
        let office_light = &things[0]; // in ROOM0002 "Office"
        assert!(already_in_room(office_light, "ROOM0002"));
        assert!(!already_in_room(office_light, "ROOM0003"));
        assert_eq!(
            previous_room(&families, office_light).unwrap().name,
            "Office"
        );
        let unassigned = &things[2];
        assert!(!already_in_room(unassigned, "ROOM0001"));
        assert!(previous_room(&families, unassigned).is_none());
    }

    #[test]
    fn create_refuses_a_duplicate_name_case_insensitively() {
        let families = fixture_families();
        let home = &families[0];
        let err = refuse_duplicate(home, "office").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("ROOM0002"), "{err}");
        assert!(refuse_duplicate(home, "Loft").is_ok());
    }

    #[test]
    fn delete_refuses_a_room_with_devices() {
        let families = fixture_families();
        let things = fixture_things();
        let office = &families[0].rooms[1];
        let err = refuse_non_empty(&things, office).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("1 device"), "{err}");
        let living = &families[0].rooms[0];
        assert!(refuse_non_empty(&things, living).is_ok());
    }

    #[test]
    fn read_back_predicates() {
        let families = fixture_families();
        let things = fixture_things();
        let light = "0000000000000000000000000000000000000001";
        assert!(placed_in(&things, light, "ROOM0002"));
        assert!(!placed_in(&things, light, "ROOM0001"), "not moved yet");
        assert!(!placed_in(&things, "no-such-thing", "ROOM0002"));

        assert_eq!(
            room_named(&families, "ROOM0002", "Office").unwrap().id,
            "ROOM0002"
        );
        assert!(
            room_named(&families, "ROOM0002", "Study").is_none(),
            "rename not applied"
        );
        assert!(
            room_named(&families, "ROOM0009", "Office").is_none(),
            "create not applied"
        );

        assert!(room_gone(&families, "ROOM0009"));
        assert!(!room_gone(&families, "ROOM0001"), "delete not applied");
    }

    #[test]
    fn device_rows_join_on_googles_id_and_prefer_the_account_alias() {
        let families = fixture_families();
        let things = fixture_things();
        let homes: Vec<&Family> = families.iter().collect();
        let alias = |id: &str| {
            (id == "0000000000000000000000000000000000000001").then(|| "Desk Light".to_string())
        };
        let rows = device_room_rows(&things, &homes, alias);
        assert_eq!(rows.len(), 3, "{rows:?}");
        // A Kasa device shared into Tapo: its Kasa id, the account alias over the nickname.
        assert_eq!(rows[0]["id"], "0000000000000000000000000000000000000001");
        assert_eq!(rows[0]["name"], "Desk Light");
        assert_eq!(rows[0]["room"], "Office");
        // Tapo's own device: the MAC without separators, the decoded nickname.
        assert_eq!(rows[1]["id"], "000000000002");
        assert_eq!(rows[1]["name"], "Kitchen Tapo Plug");
        // Tapo's own device in no room: the row stays, `room` is absent.
        assert_eq!(rows[2]["id"], "000000000003");
        assert_eq!(rows[2]["name"], "Living Room Tapo Bulb");
        assert!(rows[2].get("room").is_none(), "{:?}", rows[2]);
        // Scoped to a home none of them are in: nothing.
        let elsewhere: Family = serde_json::from_value(
            json!({"id": "FAM00009", "name": "Elsewhere", "default": false, "rooms": []}),
        )
        .unwrap();
        assert!(device_room_rows(&things, &[&elsewhere], |_| None).is_empty());
        // A roomless Kasa device shared into Tapo is the Kasa app's to file.
        let loose: Thing = serde_json::from_value(json!({
            "thingName": "0000000000000000000000000000000000000009", "familyId": "FAM00001",
            "roomId": null, "nickname": "Garage Plug", "deviceType": "SMART.KASAPLUG",
            "mac": "00:00:00:00:00:09"
        }))
        .unwrap();
        assert!(device_room_rows(&[loose], &homes, |_| None).is_empty());
        // A Tapo device in no home is the default home's: the only home, else
        // the flagged one (`default_of`, the rule `rooms move` uses).
        let homeless: Thing = serde_json::from_value(json!({
            "thingName": "TAPO_P100_9999", "familyId": null, "roomId": null,
            "nickname": "Attic Plug", "deviceType": "SMART.TAPOPLUG",
            "mac": "00:00:00:00:00:10"
        }))
        .unwrap();
        let only = std::slice::from_ref(&homeless);
        assert_eq!(device_room_rows(only, &homes, |_| None).len(), 1);
        // One selected home is the default whether or not it is flagged.
        assert_eq!(device_room_rows(only, &[&elsewhere], |_| None).len(), 1);
        // Several homes and none flagged: nothing to file it under.
        let no_default: Vec<Family> = two_homes()
            .into_iter()
            .map(|mut h| {
                h.is_default = false;
                h
            })
            .collect();
        let several: Vec<&Family> = no_default.iter().collect();
        assert!(device_room_rows(only, &several, |_| None).is_empty());
        let flagged = two_homes();
        let several: Vec<&Family> = flagged.iter().collect();
        assert_eq!(
            device_room_rows(only, &several, |_| None)[0]["id"],
            "000000000010"
        );
    }

    #[test]
    fn app_server_cache_honours_its_expiry() {
        let mut t: TokenSet = serde_json::from_value(json!({
            "token": "k", "refresh_token": null, "username": "u", "regional_url": "r", "term_id": "t",
            "tapo_token": "tt", "tapo_refresh_token": null, "tapo_regional_url": "https://example.com"
        }))
        .unwrap();
        assert!(cached_app_server(&t, 1_000).is_none(), "nothing cached");
        t.tapo_app_server_url = Some("https://use1-app-server.example.com".into());
        t.tapo_app_server_expires_at = Some(2_000);
        assert_eq!(
            cached_app_server(&t, 1_999).as_deref(),
            Some("https://use1-app-server.example.com")
        );
        assert!(
            cached_app_server(&t, 2_000).is_none(),
            "expired at the boundary"
        );
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
