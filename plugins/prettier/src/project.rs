use std::fs;
use std::path::Path;

/// Prettier's configuration files.
const CONFIGS: &[&str] = &[
    ".prettierrc",
    ".prettierrc.json",
    ".prettierrc.json5",
    ".prettierrc.yaml",
    ".prettierrc.yml",
    ".prettierrc.toml",
    ".prettierrc.js",
    ".prettierrc.cjs",
    ".prettierrc.mjs",
    ".prettierrc.ts",
    ".prettierrc.cts",
    ".prettierrc.mts",
    "prettier.config.js",
    "prettier.config.cjs",
    "prettier.config.mjs",
    "prettier.config.ts",
    "prettier.config.cts",
    "prettier.config.mts",
    "node_modules/prettier",
];

/// Whether `path`'s project formats with prettier: a configuration, a `prettier` key in
/// `package.json` or its own prettier, from the file's directory up to the Git work tree.
pub(crate) fn uses_prettier(path: &Path) -> bool {
    for directory in path.ancestors().skip(1) {
        if CONFIGS.iter().any(|config| directory.join(config).exists())
            || has_prettier_key(&directory.join("package.json"))
        {
            return true;
        }
        if directory.join(".git").exists() {
            return false;
        }
    }
    false
}

fn has_prettier_key(package: &Path) -> bool {
    fs::read_to_string(package)
        .ok()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .is_some_and(|package| package.get("prettier").is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_config_a_package_key_or_its_own_prettier_mark_a_project() {
        let root = std::env::temp_dir().join(format!("remacs-prettier-{}", std::process::id()));
        let project = |name: &str, file: &str, contents: &str| {
            let directory = root.join(name);
            fs::create_dir_all(directory.join("src")).unwrap();
            fs::create_dir_all(directory.join(".git")).unwrap();
            if !file.is_empty() {
                let file = directory.join(file);
                fs::create_dir_all(file.parent().unwrap()).unwrap();
                fs::write(file, contents).unwrap();
            }
            directory.join("src/index.ts")
        };
        assert!(uses_prettier(&project("config", ".prettierrc", "{}")));
        assert!(uses_prettier(&project(
            "key",
            "package.json",
            r#"{ "prettier": {} }"#
        )));
        assert!(uses_prettier(&project(
            "own",
            "node_modules/prettier/package.json",
            "{}"
        )));
        assert!(!uses_prettier(&project("none", "package.json", "{}")));
        fs::remove_dir_all(root).unwrap();
    }
}
