use std::env;
use std::path::PathBuf;

pub fn data_home() -> PathBuf {
    if let Some(path) = env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(path);
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".local").join("share");
    }
    env::temp_dir()
}

#[allow(dead_code)]
pub fn config_home() -> PathBuf {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path);
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".config");
    }
    env::temp_dir()
}

pub fn na_data_dir() -> PathBuf {
    data_home().join("na")
}

pub fn na_plugins_dir() -> PathBuf {
    na_data_dir().join("plugins")
}

pub fn na_backup_dir() -> PathBuf {
    na_data_dir().join("backup")
}

#[cfg(test)]
mod tests {
    use super::{config_home, data_home};

    #[test]
    fn prefers_xdg_data_home() {
        let xdg = std::env::temp_dir().join("na_rust_xdg_data_home_shared");
        std::env::set_var("XDG_DATA_HOME", &xdg);
        let out = data_home();
        std::env::remove_var("XDG_DATA_HOME");
        assert_eq!(out, xdg);
    }

    #[test]
    fn prefers_xdg_config_home() {
        let xdg = std::env::temp_dir().join("na_rust_xdg_config_home_shared");
        std::env::set_var("XDG_CONFIG_HOME", &xdg);
        let out = config_home();
        std::env::remove_var("XDG_CONFIG_HOME");
        assert_eq!(out, xdg);
    }
}
