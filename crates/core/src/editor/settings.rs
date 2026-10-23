use std::any::TypeId;
use std::path::{Path, PathBuf};

use remacs_config::{Section, Settings, project_cache_dir};

use super::Editor;
use crate::key_bindings::KeysSettings;
use crate::mode::ModesSettings;

impl Editor {
    /// Replaces the settings and lets plugins read their sections again, then refreshes,
    /// so that they act on them without waiting for a command, e.g. restart a server.
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
        let keys = self.settings_section::<KeysSettings>();
        let (keymap, errors) = keys.apply(&self.default_keymap, &self.registry.commands);
        self.registry.keymap = keymap;
        if let Some(error) = errors.into_iter().next() {
            self.set_error(error);
        }
        let modes = self.settings_section::<ModesSettings>();
        if let Some(error) = self.registry.modes.set_settings(&modes).into_iter().next() {
            self.set_error(error);
        }
        for hook in self.registry.settings_hooks.clone() {
            hook(self);
        }
        self.refresh();
    }

    /// A plugin's section of the settings; when it is wrong, its defaults, and the error
    /// is shown.
    pub fn settings_section<T: Section>(&mut self) -> T {
        self.settings
            .section::<T>()
            .unwrap_or_else(|(error, defaults)| {
                self.set_error(error.to_string());
                defaults
            })
    }

    /// State a plugin keeps between commands, created on first use.
    pub fn plugin_state<T: Default + 'static>(&mut self) -> &mut T {
        self.plugin_states
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(T::default()))
            .downcast_mut()
            .expect("plugin states are keyed by their type")
    }

    /// A plugin's state, if it has created any.
    pub fn find_plugin_state<T: 'static>(&self) -> Option<&T> {
        self.plugin_states.get(&TypeId::of::<T>())?.downcast_ref()
    }

    /// Lets plugins keep what they can rebuild under `dir`, e.g. a project's file index.
    pub fn keep_caches_in(&mut self, dir: PathBuf) {
        self.cache_dir = Some(dir);
    }

    /// The directory a plugin may keep the project at `root`'s caches in; none until
    /// [`Editor::keep_caches_in`], e.g. in tests.
    pub fn project_cache_dir(&self, root: &Path) -> Option<PathBuf> {
        let cache = self.cache_dir.as_deref()?;
        Some(project_cache_dir(cache, root))
    }
}
