use clap::Subcommand;
use serde_json::json;

use super::PowerAction;
use crate::cli::output::print_json;
use crate::config::RuntimeConfig;
use crate::error::AppError;
use crate::models::schedule::{parse_days, parse_time, ScheduleRuleBuilder};

use super::super::resolve;

#[derive(Subcommand)]
pub enum ScheduleCommand {
    /// List schedule rules
    List {
        /// Device name or ID
        device: String,
    },

    /// Get a specific schedule rule
    Get {
        /// Device name or ID
        device: String,
        /// Rule ID
        rule_id: String,
    },

    /// Add a new schedule rule
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

    /// Edit an existing schedule rule
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

    /// Delete a schedule rule
    Delete {
        /// Device name or ID
        device: String,
        /// Rule ID
        rule_id: String,
    },

    /// Delete all schedule rules
    Clear {
        /// Device name or ID
        device: String,
    },
}

pub async fn handle(cmd: &ScheduleCommand, config: &RuntimeConfig) -> Result<(), AppError> {
    match cmd {
        ScheduleCommand::List { device } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            let rules = dev.get_schedule_rules().await?;
            if let Some(rules) = rules {
                print_json(&json!({"device": dev.alias(), "rules": rules}));
            } else {
                print_json(&json!({"device": dev.alias(), "rules": []}));
            }
            Ok(())
        }
        ScheduleCommand::Get { device, rule_id } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            let rules = dev.get_schedule_rules().await?;
            if let Some(rules_data) = rules {
                if let Some(rule_list) = rules_data.get("rule_list").and_then(|v| v.as_array()) {
                    for rule in rule_list {
                        if rule.get("id").and_then(|v| v.as_str()) == Some(rule_id) {
                            print_json(&json!({"device": dev.alias(), "rule": rule}));
                            return Ok(());
                        }
                    }
                }
            }
            Err(AppError::DeviceNotFound(format!(
                "Schedule rule '{}' not found",
                rule_id
            )))
        }
        ScheduleCommand::Add {
            device,
            action,
            time,
            sunrise,
            sunset,
            days,
            name,
        } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;

            let turn_on = matches!(action, PowerAction::On);
            let mut builder = ScheduleRuleBuilder::new().with_action(turn_on);

            if let Some(name) = name {
                builder = builder.with_name(name.clone());
            }

            if *sunrise {
                builder = builder.with_sunrise();
            } else if *sunset {
                builder = builder.with_sunset();
            } else if let Some(time_str) = time {
                let (hour, minute) = parse_time(time_str)?;
                builder = builder.with_time(hour, minute);
            } else {
                return Err(AppError::InvalidInput(
                    "Specify --time HH:MM, --sunrise, or --sunset".into(),
                ));
            }

            if let Some(days) = days {
                let wday = parse_days(days)?;
                builder = builder.with_days(wday);
            }

            let rule = builder.build()?;
            let result = dev.add_schedule_rule(rule).await?;
            print_json(&json!({"device": dev.alias(), "result": result}));
            Ok(())
        }
        ScheduleCommand::Edit {
            device,
            rule_id,
            action,
            time,
            days,
            enable,
            disable,
        } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;

            // Fetch existing rule
            let rules = dev.get_schedule_rules().await?;
            let existing_rule = rules
                .as_ref()
                .and_then(|r| r.get("rule_list"))
                .and_then(|v| v.as_array())
                .and_then(|arr| {
                    arr.iter()
                        .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(rule_id))
                })
                .cloned()
                .ok_or_else(|| AppError::DeviceNotFound(format!("Rule '{}' not found", rule_id)))?;

            let mut updated = existing_rule.clone();

            if let Some(action) = action {
                updated["sact"] = json!(if matches!(action, PowerAction::On) {
                    1
                } else {
                    0
                });
            }
            if let Some(time_str) = time {
                let (hour, minute) = parse_time(time_str)?;
                updated["stime_opt"] = json!(0);
                updated["smin"] = json!((hour * 60 + minute) as i32);
            }
            if let Some(days) = days {
                let wday = parse_days(days)?;
                updated["wday"] = json!(wday);
            }
            if *enable {
                updated["enable"] = json!(1);
            }
            if *disable {
                updated["enable"] = json!(0);
            }

            let result = dev.edit_schedule_rule(updated).await?;
            print_json(&json!({"device": dev.alias(), "result": result}));
            Ok(())
        }
        ScheduleCommand::Delete { device, rule_id } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            let result = dev.delete_schedule_rule(rule_id).await?;
            print_json(&json!({"device": dev.alias(), "deleted": rule_id, "result": result}));
            Ok(())
        }
        ScheduleCommand::Clear { device } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            let result = dev.delete_all_schedule_rules().await?;
            print_json(&json!({"device": dev.alias(), "cleared": true, "result": result}));
            Ok(())
        }
    }
}
