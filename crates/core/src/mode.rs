use std::path::Path;

pub const FUNDAMENTAL_MODE: &str = "Fundamental";

#[derive(Clone, Copy)]
pub struct Mode {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    /// Brackets typed in pairs, opener first.
    pub pairs: &'static [(char, char)],
}

#[derive(Default)]
pub struct Modes(Vec<Mode>);

impl Modes {
    pub fn add(&mut self, mode: Mode) {
        self.0.push(mode);
    }

    pub fn get(&self, name: &str) -> Option<&Mode> {
        self.0.iter().find(|mode| mode.name == name)
    }

    pub fn name_for(&self, path: Option<&Path>) -> &'static str {
        let extension = path
            .and_then(Path::extension)
            .and_then(|extension| extension.to_str());
        self.0
            .iter()
            .find(|mode| extension.is_some_and(|extension| mode.extensions.contains(&extension)))
            .map_or(FUNDAMENTAL_MODE, |mode| mode.name)
    }
}
