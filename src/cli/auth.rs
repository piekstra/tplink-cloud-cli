//! `auth login|status|logout|set-credential` (SPEC v1 §1.2).
//!
//! Login exchanges the account password for Kasa and Tapo session tokens
//! and stores only the tokens. The password enters via `--stdin`,
//! `--from-env <VAR>`, `$TPLC_PASSWORD`, or a no-echo prompt — never argv.
//! A login that TP-Link stops for an emailed MFA code is *parked* in the
//! keychain (with the terminal id the code is bound to) and resumed with
//! `--mfa-code`; non-interactively that parking is reported as an
//! `mfa_required` DTO with exit 3.

use pk_cli_auth::{AuthMethod, AuthStatus, LogoutArgs, SetCredentialArgs};
use pk_cli_core::output::{self, emit_one};
use pk_cli_core::CliError;
use pk_cli_secrets::Secret;
use serde_json::{json, Map, Value};

use super::emit::Ctx;
use crate::api::client::{LoginResult, TPLinkApi};
use crate::api::cloud_type::CloudType;
use crate::config::Config;
use crate::error::AppError;
use crate::session::{PendingLogin, TokenSet};

/// `auth login` flags: the family's standard set plus the identity and the
/// MFA resume code. `--no-verify` is accepted for family uniformity and has
/// no effect — the login itself is the verification.
#[derive(clap::Args, Debug, Clone)]
pub struct LoginOpts {
    #[command(flatten)]
    pub base: pk_cli_auth::LoginArgs,
    /// Account email (else `config set username`, then a prompt).
    #[arg(long, env = "TPLC_USERNAME")]
    pub username: Option<String>,
    /// The code TP-Link emailed, to resume a login that stopped for MFA.
    #[arg(long, value_name = "CODE")]
    pub mfa_code: Option<String>,
}

/// Exit code for a login that parked for an MFA code (SPEC §1.5: auth
/// required). The `mfa_required` DTO has already been emitted.
pub const EXIT_MFA_PARKED: i32 = 3;

fn prompt_line(label: &str) -> Result<String, CliError> {
    eprint!("{label}: ");
    let mut s = String::new();
    std::io::stdin()
        .read_line(&mut s)
        .map_err(|e| CliError::Other(format!("reading {label}: {e}")))?;
    let s = s.trim().to_string();
    if s.is_empty() {
        return Err(CliError::Usage(format!("{label} is required")));
    }
    Ok(s)
}

/// The password, by precedence: `--stdin`/`--from-env` > `$TPLC_PASSWORD` >
/// no-echo prompt (interactive only). Never argv.
fn read_password(args: &pk_cli_auth::LoginArgs, prompt_ok: bool) -> Result<Secret, CliError> {
    if args.source.stdin || args.source.from_env.is_some() {
        return args.source.read(None);
    }
    if let Ok(p) = std::env::var("TPLC_PASSWORD") {
        if !p.is_empty() {
            return Ok(Secret::new(p));
        }
    }
    if prompt_ok {
        return Secret::prompt("Password");
    }
    Err(CliError::Usage(
        "no password: pass --stdin or --from-env <VAR> (or set TPLC_PASSWORD)".into(),
    ))
}

/// Returns the process exit code: 0 when logged in, [`EXIT_MFA_PARKED`]
/// when the login parked for an MFA code.
pub async fn login(ctx: &Ctx<'_>, args: &LoginOpts) -> Result<i32, CliError> {
    let prompt_ok = ctx.interactive && !args.base.non_interactive;

    // Identity and secret first: every usage error here happens before the
    // keychain or the network is touched. Only the resume path (`--mfa-code`
    // with no identity given) may consult the parked login for its username.
    let mut username = args
        .username
        .clone()
        .filter(|u| !u.is_empty())
        .or_else(|| ctx.cfg.username.clone());
    if username.is_none() && args.mfa_code.is_none() {
        if prompt_ok {
            username = Some(prompt_line("TP-Link email")?);
        } else {
            return Err(CliError::Usage(
                "no account: pass --username, set TPLC_USERNAME, or run `tplc config set username you@example.com`".into(),
            ));
        }
    }
    let password = read_password(&args.base, prompt_ok)?;

    let pending = ctx.sessions.pending()?;
    let username = match (username, &pending) {
        (Some(u), _) => u,
        (None, Some(p)) => p.username.clone(),
        (None, None) => return Err(CliError::Usage(
            "no login is parked for --mfa-code; pass --username (or run `tplc auth login` first)"
                .into(),
        )),
    };
    let resume = pending.filter(|p| p.username == username);

    // Logging in again as the same account simply renews the session; a
    // different account replaces someone else's and needs --overwrite.
    if !args.base.overwrite {
        if let Some(existing) = ctx.sessions.load()? {
            if !existing.token.is_empty() && existing.username != username {
                return Err(CliError::Usage(format!(
                    "a session for {} is already stored; pass --overwrite to replace it (or `tplc auth logout` first)",
                    existing.username
                )));
            }
        }
    }

    let term_id = resume.as_ref().map(|p| p.term_id.clone());
    let mut kasa_api = TPLinkApi::new(None, ctx.verbose, term_id, CloudType::Kasa)?;

    // A code on the command line is only meaningful for the cloud that asked.
    let waiting = resume.as_ref().map(|p| p.cloud);
    let code_for = |cloud: CloudType| -> Option<&str> {
        args.mfa_code
            .as_deref()
            .filter(|_| waiting.is_none_or(|w| w == cloud))
    };

    let kasa = match &resume {
        Some(p) if p.cloud == CloudType::Tapo && p.kasa_token.is_some() => LoginResult {
            token: p.kasa_token.clone().unwrap_or_default(),
            refresh_token: p.kasa_refresh_token.clone(),
            regional_url: p.kasa_regional_url.clone().unwrap_or_default(),
        },
        _ => {
            let attempt = match code_for(CloudType::Kasa) {
                Some(code) => {
                    kasa_api
                        .verify_mfa(&username, password.expose(), code)
                        .await
                }
                None => kasa_api.login(&username, password.expose()).await,
            };
            match attempt {
                Ok(result) => result,
                Err(AppError::MfaRequired { email, .. }) => {
                    if prompt_ok {
                        let code = prompt_mfa(CloudType::Kasa, email.as_deref())?;
                        kasa_api
                            .verify_mfa(&username, password.expose(), &code)
                            .await?
                    } else {
                        let park = PendingLogin {
                            username: username.clone(),
                            term_id: kasa_api.term_id().to_string(),
                            cloud: CloudType::Kasa,
                            kasa_token: None,
                            kasa_refresh_token: None,
                            kasa_regional_url: None,
                        };
                        return park_for_mfa(ctx, park, email);
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
    };

    // Tapo, same terminal id. A missing Tapo session is not fatal: the
    // account may simply have no Tapo devices.
    let mut tapo_api = TPLinkApi::new(
        None,
        ctx.verbose,
        Some(kasa_api.term_id().to_string()),
        CloudType::Tapo,
    )?;
    let tapo_attempt = match code_for(CloudType::Tapo) {
        Some(code) => {
            tapo_api
                .verify_mfa(&username, password.expose(), code)
                .await
        }
        None => tapo_api.login(&username, password.expose()).await,
    };
    let tapo = match tapo_attempt {
        Ok(result) => Some(result),
        Err(AppError::MfaRequired { email, .. }) => {
            if prompt_ok {
                let code = prompt_mfa(CloudType::Tapo, email.as_deref())?;
                match tapo_api
                    .verify_mfa(&username, password.expose(), &code)
                    .await
                {
                    Ok(result) => Some(result),
                    Err(e) => {
                        if !ctx.quiet {
                            eprintln!("Tapo MFA failed (non-fatal): {e}");
                        }
                        None
                    }
                }
            } else {
                let park = PendingLogin {
                    username: username.clone(),
                    term_id: kasa_api.term_id().to_string(),
                    cloud: CloudType::Tapo,
                    kasa_token: Some(kasa.token.clone()),
                    kasa_refresh_token: kasa.refresh_token.clone(),
                    kasa_regional_url: Some(kasa.regional_url.clone()),
                };
                return park_for_mfa(ctx, park, email);
            }
        }
        Err(e) => {
            if ctx.verbose {
                eprintln!("Tapo login failed (non-fatal): {e}");
            }
            None
        }
    };
    drop(password);

    let tokens = TokenSet {
        token: kasa.token,
        refresh_token: kasa.refresh_token,
        username: username.clone(),
        regional_url: kasa.regional_url.clone(),
        term_id: kasa_api.term_id().to_string(),
        tapo_token: tapo.as_ref().map(|r| r.token.clone()),
        tapo_refresh_token: tapo.as_ref().and_then(|r| r.refresh_token.clone()),
        tapo_regional_url: tapo.as_ref().map(|r| r.regional_url.clone()),
        tapo_app_server_url: None,
        tapo_app_server_expires_at: None,
    };
    ctx.sessions.store(&tokens)?;
    ctx.sessions.clear_pending()?;

    // Persist the identity the session belongs to (non-secret).
    let mut cfg: Config = ctx.store.load()?;
    cfg.username = Some(username.clone());
    ctx.store.save(&cfg)?;

    if !ctx.quiet {
        eprintln!(
            "session stored in the OS keychain ({})",
            ctx.sessions.service()
        );
    }
    let mut dto = json!({
        "status": "authenticated",
        "username": username,
        "kasa_regional_url": kasa.regional_url,
        "tapo_authenticated": tapo.is_some(),
    });
    if let Some(t) = &tapo {
        dto["tapo_regional_url"] = json!(t.regional_url);
    }
    emit_one(ctx.json, "auth-login", dto);
    Ok(0)
}

fn prompt_mfa(cloud: CloudType, email: Option<&str>) -> Result<String, CliError> {
    eprintln!(
        "{cloud} MFA verification required{}",
        email
            .map(|e| format!(" (code emailed to {e})"))
            .unwrap_or_default()
    );
    prompt_line(&format!("{cloud} MFA code"))
}

/// Store the parked login and report it. The DTO is the command's output
/// (one document per invocation), so the human line goes to stderr here and
/// the caller exits [`EXIT_MFA_PARKED`] without going through `output::fail`.
fn park_for_mfa(ctx: &Ctx<'_>, park: PendingLogin, email: Option<String>) -> Result<i32, CliError> {
    let cloud = park.cloud;
    ctx.sessions.store_pending(&park)?;
    emit_one(
        ctx.json,
        "auth-login",
        json!({
            "status": "mfa_required",
            "cloud": cloud,
            "email": email,
            "next": "re-run `tplc auth login --mfa-code <CODE>` with the code TP-Link emailed",
        }),
    );
    eprintln!(
        "error: {cloud} MFA code required — re-run `tplc auth login --mfa-code <CODE>` with the code TP-Link emailed"
    );
    Ok(EXIT_MFA_PARKED)
}

/// `auth status` — works logged out. `session_valid` reports whether a
/// refresh token is stored (the session can renew itself); token liveness is
/// only knowable by calling the cloud, which a status probe must not do.
pub fn status(ctx: &Ctx<'_>) -> Result<(), CliError> {
    let session = ctx.sessions.load()?.filter(|t| !t.token.is_empty());
    let mut st = AuthStatus::new(true, session.is_some(), AuthMethod::Password);
    let mut extra: Map<String, Value> = Map::new();
    match &session {
        Some(t) => {
            st.username = Some(t.username.clone());
            st.credential_in_keychain = Some(true);
            st.session_valid = Some(t.refresh_token.is_some());
            extra.insert("kasa_regional_url".into(), json!(t.regional_url));
            extra.insert("tapo_authenticated".into(), json!(t.has_tapo()));
            if let Some(u) = &t.tapo_regional_url {
                extra.insert("tapo_regional_url".into(), json!(u));
            }
        }
        None => {
            st.username = ctx.cfg.username.clone();
            st.credential_in_keychain = Some(false);
            if let Some(p) = ctx.sessions.pending()? {
                st.username = Some(p.username.clone());
                extra.insert("mfa_pending".into(), json!({ "cloud": p.cloud }));
            }
        }
    }
    if ctx.json {
        let mut v = st.to_json();
        if let Some(map) = v.as_object_mut() {
            map.extend(extra);
        }
        output::json(&v);
    } else {
        st.render();
        for (k, v) in &extra {
            println!("{k}: {}", output::scalar(v));
        }
    }
    Ok(())
}

pub fn logout(ctx: &Ctx<'_>, args: &LogoutArgs) -> Result<(), CliError> {
    ctx.sessions.clear()?;
    if args.forget {
        ctx.store.clear()?;
    }
    if ctx.json {
        output::json(&json!({
            "schema": "auth-logout/v1",
            "status": "logged_out",
            "forgot": args.forget,
        }));
    } else if !ctx.quiet {
        eprintln!("logged out");
    }
    Ok(())
}

/// The stored secret is a session minted by the cloud, not a credential a
/// caller can supply: there is no shape to write raw, and the password is
/// deliberately never stored. Always a usage error pointing at the real path.
pub fn set_credential(_args: &SetCredentialArgs) -> Result<(), CliError> {
    Err(CliError::Usage(
        "tplc stores a login session, not a raw credential; run `tplc auth login --stdin --username you@example.com` (password on stdin) instead".into(),
    ))
}
