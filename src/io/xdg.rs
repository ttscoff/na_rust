use std::env;
use std::path::PathBuf;

#[cfg(test)]
pub static TEST_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn data_home() -> PathBuf {
    if let Some(path) = env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(path);
    }
    if let Some(home) = env::var_os("HOME") {
        #[cfg(target_os = "macos")]
        {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support");
        }
        #[cfg(not(target_os = "macos"))]
        {
            return PathBuf::from(home).join(".local").join("share");
        }
    }
    env::temp_dir()
}

#[allow(dead_code)]
pub fn config_home() -> PathBuf {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path);
    }
    if let Some(home) = env::var_os("HOME") {
        #[cfg(target_os = "macos")]
        {
            return PathBuf::from(home).join("Library").join("Preferences");
        }
        #[cfg(not(target_os = "macos"))]
        {
            return PathBuf::from(home).join(".config");
        }
    }
    env::temp_dir()
}

pub fn na_data_dir() -> PathBuf {
    data_home().join("na")
}

#[allow(dead_code)]
pub fn na_config_dir() -> PathBuf {
    config_home().join("na")
}

pub fn na_plugins_dir() -> PathBuf {
    na_data_dir().join("plugins")
}

pub fn na_backup_dir() -> PathBuf {
    na_data_dir().join("backup")
}

#[cfg(test)]
mod tests {
    use super::{config_home, data_home, na_config_dir, na_data_dir, TEST_ENV_MUTEX};

    #[test]
    fn prefers_xdg_data_home() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let xdg = std::env::temp_dir().join("na_rust_xdg_data_home_shared");
        std::env::set_var("XDG_DATA_HOME", &xdg);
        let out = data_home();
        std::env::remove_var("XDG_DATA_HOME");
        assert_eq!(out, xdg);
    }

    #[test]
    fn prefers_xdg_config_home() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let xdg = std::env::temp_dir().join("na_rust_xdg_config_home_shared");
        std::env::set_var("XDG_CONFIG_HOME", &xdg);
        let out = config_home();
        std::env::remove_var("XDG_CONFIG_HOME");
        assert_eq!(out, xdg);
    }

    #[test]
    fn na_dirs_are_under_homes() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let xdg_data = std::env::temp_dir().join("na_rust_xdg_data_dir_shared");
        let xdg_config = std::env::temp_dir().join("na_rust_xdg_config_dir_shared");
        std::env::set_var("XDG_DATA_HOME", &xdg_data);
        std::env::set_var("XDG_CONFIG_HOME", &xdg_config);
        assert_eq!(na_data_dir(), xdg_data.join("na"));
        assert_eq!(na_config_dir(), xdg_config.join("na"));
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("XDG_CONFIG_HOME");
    }
}
