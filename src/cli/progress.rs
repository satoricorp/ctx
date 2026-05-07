use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::time::Duration;

pub struct CliSpinner {
    bar: ProgressBar,
    finished: bool,
}

impl CliSpinner {
    pub fn new(message: impl Into<String>) -> Self {
        let bar = ProgressBar::new_spinner();
        bar.set_draw_target(ProgressDrawTarget::stderr());
        bar.set_style(
            ProgressStyle::with_template("{spinner} {msg}")
                .expect("valid spinner template")
                .tick_strings(&["-", "\\", "|", "/"]),
        );
        bar.enable_steady_tick(Duration::from_millis(80));
        bar.set_message(message.into());
        Self {
            bar,
            finished: false,
        }
    }

    pub fn success(mut self, message: impl Into<String>) {
        self.finished = true;
        self.bar
            .finish_with_message(format!("\x1b[32m●\x1b[0m {}", message.into()));
    }

    pub fn clear(mut self) {
        self.finished = true;
        self.bar.finish_and_clear();
    }
}

impl Drop for CliSpinner {
    fn drop(&mut self) {
        if !self.finished {
            self.bar.finish_and_clear();
        }
    }
}
