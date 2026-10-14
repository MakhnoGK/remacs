//! The commands a shell ran, from what it reported: when each started and how it ended,
//! by the prompt it was typed at.

use std::collections::BTreeMap;
use std::time::SystemTime;

use remacs_core::RowMark;

use crate::scan::Mark;

/// Older ones are forgotten; their rows have long left the scrollback.
const KEPT_COMMANDS: usize = 2_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Command {
    started: Option<SystemTime>,
    finished: Option<(Option<i32>, SystemTime)>,
}

#[derive(Default)]
pub struct Commands {
    by_prompt: BTreeMap<u64, Command>,
    /// The prompt shown last, which the next command is typed at.
    current: Option<u64>,
}

impl Commands {
    pub fn apply(&mut self, mark: Mark, at: SystemTime) {
        match mark {
            Mark::Prompt(id) => {
                self.current = Some(id);
                self.by_prompt.insert(id, Command::default());
                while self.by_prompt.len() > KEPT_COMMANDS {
                    self.by_prompt.pop_first();
                }
            }
            Mark::Started => {
                if let Some(command) = self.current_mut() {
                    command.started = Some(at);
                }
            }
            Mark::Finished(code) => {
                if let Some(command) = self.current_mut()
                    && command.started.is_some()
                {
                    command.finished = Some((code, at));
                }
            }
        }
    }

    /// Whether a command typed at the last prompt runs now.
    pub fn running(&self) -> bool {
        self.current
            .and_then(|id| self.by_prompt.get(&id))
            .is_some_and(|command| command.started.is_some() && command.finished.is_none())
    }

    fn current_mut(&mut self) -> Option<&mut Command> {
        self.by_prompt.get_mut(&self.current?)
    }

    /// The mark beside the prompt `id`: how its command ended, or that it runs; none
    /// for a prompt no command ran from.
    pub fn mark(&self, id: u64) -> Option<RowMark> {
        let command = self.by_prompt.get(&id)?;
        let started = command.started?;
        let Some((code, finished)) = command.finished else {
            return Some(RowMark {
                exit: None,
                detail: "running".to_owned(),
            });
        };
        let code = code.unwrap_or(0);
        let took = finished
            .duration_since(started)
            .unwrap_or_default()
            .as_secs_f32();
        let detail = format!(
            "exit {code} · {} · finished {}",
            duration(took),
            clock(finished)
        );
        Some(RowMark {
            exit: Some(code),
            detail,
        })
    }
}

fn duration(seconds: f32) -> String {
    if seconds < 60.0 {
        format!("{seconds:.1} s")
    } else {
        let whole = seconds as u64;
        format!("{}:{:02}", whole / 60, whole % 60)
    }
}

/// `10:41` in local time.
fn clock(at: SystemTime) -> String {
    let seconds = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as libc::time_t;
    // SAFETY: `tm` is plain data, valid zeroed.
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are to locals that outlive the call.
    unsafe { libc::localtime_r(&seconds, &mut local) };
    format!("{}:{:02}", local.tm_hour, local.tm_min)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn marks_commands_by_their_prompt() {
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut commands = Commands::default();
        commands.apply(Mark::Prompt(1), start);
        assert_eq!(commands.mark(1), None);
        assert!(!commands.running());
        commands.apply(Mark::Started, start);
        assert_eq!(commands.mark(1).unwrap().exit, None);
        assert!(commands.running());
        commands.apply(
            Mark::Finished(Some(101)),
            start + Duration::from_millis(2_800),
        );
        let mark = commands.mark(1).unwrap();
        assert_eq!(mark.exit, Some(101));
        assert!(!commands.running());
        assert!(mark.detail.starts_with("exit 101 · 2.8 s · finished "));
        // An empty command line runs nothing.
        commands.apply(Mark::Prompt(2), start);
        commands.apply(Mark::Finished(Some(0)), start);
        assert_eq!(commands.mark(2), None);
    }
}
