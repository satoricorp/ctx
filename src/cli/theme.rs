use std::fmt::Display;

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const CYAN: &str = "\x1b[36m";
pub const CYAN_BOLD: &str = "\x1b[36;1m";
pub const GREEN: &str = "\x1b[32m";
pub const GREEN_BOLD: &str = "\x1b[32;1m";
pub const AMBER_BOLD: &str = "\x1b[33;1m";
pub const MAGENTA: &str = "\x1b[35m";
pub const MUTED: &str = "\x1b[90m";
pub const WHITE_BOLD: &str = "\x1b[37;1m";

pub const CTX_BANNER: &str = concat!(
    "\x1b[36;1m   ██████╗████████╗██╗  ██╗\n",
    "  ██╔════╝╚══██╔══╝╚██╗██╔╝\n",
    "  ██║        ██║    ╚███╔╝\n",
    "  ██║        ██║    ██╔██╗\n",
    "  ╚██████╗   ██║   ██╔╝ ██╗\n",
    "   ╚═════╝   ╚═╝   ╚═╝  ╚═╝\x1b[0m\n",
    "\x1b[90m   local-first context runtime for agents\x1b[0m\n",
    "\x1b[2m   remember everything. retrieve exactly what matters.\x1b[0m",
);

pub fn headline(action: impl Display, target: impl Display) -> String {
    format!("{WHITE_BOLD}{action}{RESET} {CYAN_BOLD}{target}{RESET}")
}

pub fn headline_detail(action: impl Display, target: impl Display, detail: impl Display) -> String {
    format!("{} {MUTED}{detail}{RESET}", headline(action, target))
}

pub fn success(action: impl Display, target: impl Display) -> String {
    format!("{GREEN_BOLD}●{RESET} {}", headline(action, target))
}

pub fn success_detail(action: impl Display, target: impl Display, detail: impl Display) -> String {
    format!("{} {MUTED}{detail}{RESET}", success(action, target))
}

pub fn warn(message: impl Display) -> String {
    format!("{AMBER_BOLD}!{RESET} {message}")
}

pub fn section(title: impl Display) -> String {
    format!("{AMBER_BOLD}{title}{RESET}")
}

pub fn bullet(value: impl Display) -> String {
    format!("{GREEN}•{RESET} {CYAN}{value}{RESET}")
}

pub fn key_value(key: impl Display, value: impl Display) -> String {
    format!("{MUTED}{key}:{RESET} {value}")
}

pub fn pill(value: impl Display) -> String {
    format!("{GREEN}({value}){RESET}")
}

pub fn muted(value: impl Display) -> String {
    format!("{MUTED}{value}{RESET}")
}

pub fn command(value: impl Display) -> String {
    format!("{CYAN_BOLD}{value}{RESET}")
}
