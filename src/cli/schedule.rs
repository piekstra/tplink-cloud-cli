//! `schedule list|get|add|edit|delete|clear` — a device's on/off rules.
//! Arguments (times, days) are validated before the device is resolved, so
//! bad input never reaches the keychain or the network.

use clap::Subcommand;
use pk_cli_core::CliError;
use serde_json::{json, Map, Value};

use pk_cli_core::output::emit_one;

use super::emit::{emit_headed_list, Ctx};
use super::PowerAction;
use crate::error::AppError;
use crate::models::schedule::{parse_days, parse_time, ScheduleRuleBuilder};
use crate::resolve;

pub const COLUMNS: &[&str] = &["id", "name", "enabled", "action", "time", "days"];

#[derive(Subcommand, Debug)]
pub enum ScheduleCommand {
    /// List a device's schedule rules (schedule-rule-list/v1).
    #[command(visible_alias = "ls")]
    List {
        /// Device name or ID
        device: String,
    },
    /// One rule by id (schedule-rule/v1).
    Get {
        /// Device name or ID
        device: String,
        /// Rule ID
        rule_id: String,
    },
    /// Add a rule.
    Add {
        /// Device name or ID
        device: String,
        /// Action: on or off
        #[arg(long, value_enum)]
        action: PowerAction,
        /// Time in HH:MM format
        #[arg(long, conflicts_with_all = ["sunrise", "sunset"])]
        time: Option<String>,
        /// Trigger at sunrise
        #[arg(long, conflicts_with_all = ["time", "sunset"])]
        sunrise: bool,
        /// Trigger at sunset
        #[arg(long, conflicts_with_all = ["time", "sunrise"])]
        sunset: bool,
        /// Days of week (comma-separated: mon,tue,wed,thu,fri,sat,sun)
        #[arg(long, value_delimiter = ',')]
        days: Option<Vec<String>>,
        /// Rule name
        #[arg(long)]
        name: Option<String>,
    },
    /// Edit an existing rule.
    Edit {
        /// Device name or ID
        device: String,
        /// Rule ID
        rule_id: String,
        /// Action: on or off
        #[arg(long, value_enum)]
        action: Option<PowerAction>,
        /// Time in HH:MM format
        #[arg(long)]
        time: Option<String>,
        /// Days of week (comma-separated)
        #[arg(long, value_delimiter = ',')]
        days: Option<Vec<String>>,
        /// Enable the rule
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        /// Disable the rule
        #[arg(long, conflicts_with = "enable")]
        disable: bool,
    },
    /// Delete one rule.
    Delete {
        /// Device name or ID
        device: String,
        /// Rule ID
        rule_id: String,
    },
    /// Delete every rule on the device.
    Clear {
        /// Device name or ID
        device: String,
    },
}

/// What `add`/`edit` will send, resolved from the flags before any I/O.
struct Change {
    turn_on: Option<bool>,
    time: Option<(u32, u32)>,
    start: Option<Start>,
    days: Option<Vec<i32>>,
}

enum Start {
    Sunrise,
    Sunset,
}

/// Validate the rule-shaping flags. Called before the device is resolved.
pub fn validate(cmd: &ScheduleCommand) -> Result<(), CliError> {
    change_for(cmd).map(|_| ())
}

fn change_for(cmd: &ScheduleCommand) -> Result<Option<Change>, CliError> {
    match cmd {
        ScheduleCommand::Add {
            action,
            time,
            sunrise,
            sunset,
            days,
            ..
        } => {
            let start = if *sunrise {
                Some(Start::Sunrise)
            } else if *sunset {
                Some(Start::Sunset)
            } else {
                None
            };
            let time = time.as_deref().map(parse_time).transpose()?;
            if start.is_none() && time.is_none() {
                return Err(AppError::InvalidInput(
                    "specify --time HH:MM, --sunrise, or --sunset".into(),
                )
                .into());
            }
            Ok(Some(Change {
                turn_on: Some(matches!(action, PowerAction::On)),
                time,
                start,
                days: days.as_deref().map(parse_days).transpose()?,
            }))
        }
        ScheduleCommand::Edit {
            action, time, days, ..
        } => Ok(Some(Change {
            turn_on: action.map(|a| matches!(a, PowerAction::On)),
            time: time.as_deref().map(parse_time).transpose()?,
            start: None,
            days: days.as_deref().map(parse_days).transpose()?,
        })),
        _ => Ok(None),
    }
}

const DAY_NAMES: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

/// A rule as the CLI presents it: the device's fields plus readable
/// `enabled`/`action`/`time`/`days` derived from them.
pub fn rule_row(rule: &Value) -> Value {
    let int = |k: &str| rule.get(k).and_then(Value::as_i64);
    let time = match (int("stime_opt"), int("smin")) {
        (Some(1), _) => Some("sunrise".to_string()),
        (Some(2), _) => Some("sunset".to_string()),
        (Some(0), Some(m)) => Some(format!("{:02}:{:02}", m / 60, m % 60)),
        _ => None,
    };
    let days = rule.get("wday").and_then(Value::as_array).map(|w| {
        let on: Vec<&str> = w
            .iter()
            .enumerate()
            .filter(|(_, v)| v.as_i64() == Some(1))
            .filter_map(|(i, _)| DAY_NAMES.get(i).copied())
            .collect();
        if on.len() == 7 {
            "daily".to_string()
        } else {
            on.join(",")
        }
    });
    let mut row = Map::new();
    row.insert("id".into(), rule.get("id").cloned().unwrap_or(Value::Null));
    row.insert(
        "name".into(),
        rule.get("name").cloned().unwrap_or(Value::Null),
    );
    row.insert("enabled".into(), json!(int("enable") == Some(1)));
    row.insert(
        "action".into(),
        json!(match int("sact") {
            Some(1) => "on",
            Some(0) => "off",
            _ => "none",
        }),
    );
    if let Some(t) = time {
        row.insert("time".into(), json!(t));
    }
    if let Some(d) = days {
        row.insert("days".into(), json!(d));
    }
    row.insert("repeat".into(), json!(int("repeat") == Some(1)));
    row.insert("rule".into(), rule.clone());
    Value::Object(row)
}

fn rule_list(rules: Option<Value>) -> Vec<Value> {
    rules
        .as_ref()
        .and_then(|r| r.get("rule_list"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn find_rule(rules: &[Value], rule_id: &str) -> Option<Value> {
    rules
        .iter()
        .find(|r| r.get("id").and_then(Value::as_str) == Some(rule_id))
        .cloned()
}

pub async fn handle(ctx: &Ctx<'_>, cmd: &ScheduleCommand) -> Result<(), CliError> {
    let change = change_for(cmd)?;
    let device = match cmd {
        ScheduleCommand::List { device }
        | ScheduleCommand::Get { device, .. }
        | ScheduleCommand::Add { device, .. }
        | ScheduleCommand::Edit { device, .. }
        | ScheduleCommand::Delete { device, .. }
        | ScheduleCommand::Clear { device } => device,
    };
    let dev = resolve::resolve_device(ctx, device).await?;
    match cmd {
        ScheduleCommand::List { .. } => {
            let items = rule_list(dev.get_schedule_rules().await?)
                .iter()
                .map(rule_row)
                .collect();
            let mut head = Map::new();
            head.insert("device".into(), json!(dev.alias()));
            emit_headed_list(ctx.json, "schedule-rule", head, items, COLUMNS);
            Ok(())
        }
        ScheduleCommand::Get { rule_id, .. } => {
            let rules = rule_list(dev.get_schedule_rules().await?);
            let rule = find_rule(&rules, rule_id).ok_or_else(|| {
                AppError::DeviceNotFound(format!("no schedule rule `{rule_id}` on {}", dev.alias()))
            })?;
            let mut dto = json!({"device": dev.alias()});
            if let (Some(obj), Value::Object(r)) = (dto.as_object_mut(), rule_row(&rule)) {
                obj.extend(r);
            }
            emit_one(ctx.json, "schedule-rule", dto);
            Ok(())
        }
        ScheduleCommand::Add { name, .. } => {
            let change = change.expect("add always has a change");
            let mut builder =
                ScheduleRuleBuilder::new().with_action(change.turn_on.unwrap_or(true));
            if let Some(name) = name {
                builder = builder.with_name(name.clone());
            }
            builder = match (change.start, change.time) {
                (Some(Start::Sunrise), _) => builder.with_sunrise(),
                (Some(Start::Sunset), _) => builder.with_sunset(),
                (None, Some((h, m))) => builder.with_time(h, m),
                (None, None) => unreachable!("validated by change_for"),
            };
            if let Some(days) = change.days {
                builder = builder.with_days(days);
            }
            let rule = builder.build()?;
            let result = dev.add_schedule_rule(rule).await?;
            let rule_id = result
                .as_ref()
                .and_then(|r| r.get("id"))
                .cloned()
                .unwrap_or(Value::Null);
            emit_one(
                ctx.json,
                "schedule-change",
                json!({"device": dev.alias(), "action": "added", "rule_id": rule_id, "result": result}),
            );
            Ok(())
        }
        ScheduleCommand::Edit {
            rule_id,
            enable,
            disable,
            ..
        } => {
            let change = change.expect("edit always has a change");
            let rules = rule_list(dev.get_schedule_rules().await?);
            let mut updated = find_rule(&rules, rule_id).ok_or_else(|| {
                AppError::DeviceNotFound(format!("no schedule rule `{rule_id}` on {}", dev.alias()))
            })?;
            if let Some(on) = change.turn_on {
                updated["sact"] = json!(if on { 1 } else { 0 });
            }
            if let Some((h, m)) = change.time {
                updated["stime_opt"] = json!(0);
                updated["smin"] = json!((h * 60 + m) as i32);
            }
            if let Some(days) = change.days {
                updated["wday"] = json!(days);
            }
            if *enable {
                updated["enable"] = json!(1);
            }
            if *disable {
                updated["enable"] = json!(0);
            }
            let result = dev.edit_schedule_rule(updated).await?;
            emit_one(
                ctx.json,
                "schedule-change",
                json!({"device": dev.alias(), "action": "edited", "rule_id": rule_id, "result": result}),
            );
            Ok(())
        }
        ScheduleCommand::Delete { rule_id, .. } => {
            let result = dev.delete_schedule_rule(rule_id).await?;
            emit_one(
                ctx.json,
                "schedule-change",
                json!({"device": dev.alias(), "action": "deleted", "rule_id": rule_id, "result": result}),
            );
            Ok(())
        }
        ScheduleCommand::Clear { .. } => {
            let result = dev.delete_all_schedule_rules().await?;
            emit_one(
                ctx.json,
                "schedule-change",
                json!({"device": dev.alias(), "action": "cleared", "result": result}),
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_rows_derive_readable_fields() {
        let r = json!({"id": "A1", "name": "Morning", "enable": 1, "sact": 1, "stime_opt": 0,
                       "smin": 425, "wday": [0,1,1,1,1,1,0], "repeat": 1});
        let row = rule_row(&r);
        assert_eq!(row["time"], "07:05");
        assert_eq!(row["days"], "mon,tue,wed,thu,fri");
        assert_eq!(row["action"], "on");
        assert_eq!(row["enabled"], true);
        assert_eq!(row["rule"]["smin"], 425);

        let sunset = json!({"id": "B", "enable": 0, "sact": 0, "stime_opt": 2, "smin": 0,
                            "wday": [1,1,1,1,1,1,1]});
        let row = rule_row(&sunset);
        assert_eq!(row["time"], "sunset");
        assert_eq!(row["days"], "daily");
        assert_eq!(row["action"], "off");
        assert_eq!(row["enabled"], false);
    }

    fn add(time: Option<&str>, sunrise: bool, days: Option<Vec<&str>>) -> ScheduleCommand {
        ScheduleCommand::Add {
            device: "x".into(),
            action: PowerAction::On,
            time: time.map(String::from),
            sunrise,
            sunset: false,
            days: days.map(|d| d.into_iter().map(String::from).collect()),
            name: None,
        }
    }

    #[test]
    fn add_is_validated_before_any_io() {
        assert!(validate(&add(None, false, None)).is_err());
        assert!(validate(&add(Some("25:00"), false, None)).is_err());
        assert!(validate(&add(Some("07:00"), false, Some(vec!["funday"]))).is_err());
        assert!(validate(&add(Some("07:00"), false, Some(vec!["mon", "fri"]))).is_ok());
        assert!(validate(&add(None, true, None)).is_ok());
        let e = validate(&add(None, false, None)).unwrap_err();
        assert_eq!(e.exit_code(), 2);
    }
}
