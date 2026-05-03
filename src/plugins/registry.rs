use crate::io::xdg::na_plugins_dir;
use crate::models::action::Action;
use crate::plugins::format::{serialize_actions_with_divider, PluginDataFormat};
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct Plugin {
    pub name: String,
    pub path: PathBuf,
    pub input_format: PluginDataFormat,
    pub output_format: PluginDataFormat,
}

#[derive(Debug)]
pub struct PluginRegistry {
    pub plugins: Vec<Plugin>,
}

pub fn load_plugin_from_path(path: &Path) -> Result<Plugin> {
    if !path.is_file() {
        return Err(anyhow!("Not a plugin file: {:?}", path));
    }
    let name = path
        .file_stem()
        .and_then(|x| x.to_str())
        .unwrap_or("unknown")
        .to_string();
    let (input_format, output_format) = parse_format_hints(path)?;
    Ok(Plugin {
        name,
        path: path.to_path_buf(),
        input_format,
        output_format,
    })
}

impl PluginRegistry {
    pub fn default_dir() -> Result<PathBuf> {
        let dir = na_plugins_dir();
        if dir.as_os_str().is_empty() {
            return Err(anyhow!("Unable to resolve plugin directory"));
        }
        Ok(dir)
    }

    pub fn discover(dir: &Path) -> Result<Self> {
        if !dir.exists() {
            return Ok(Self {
                plugins: Vec::new(),
            });
        }

        let mut plugins = Vec::new();
        for entry in WalkDir::new(dir).max_depth(1) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let meta = fs::metadata(entry.path())?;
            #[cfg(unix)]
            if {
                use std::os::unix::fs::PermissionsExt;
                meta.permissions().mode() & 0o111 == 0
            } {
                continue;
            }

            let path = entry.path().to_path_buf();
            let name = path
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or("unknown")
                .to_string();
            let (input_format, output_format) = parse_format_hints(&path)?;
            plugins.push(Plugin {
                name,
                path,
                input_format,
                output_format,
            });
        }
        plugins.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Self { plugins })
    }

    pub fn plugin(&self, name: &str) -> Result<PluginRunner> {
        let plugin = self
            .plugins
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| anyhow!("Plugin not found: {name}"))?;
        Ok(PluginRunner::new(plugin.clone()))
    }

    /// Resolve by basename in the registry or by direct executable path.
    pub fn resolve_plugin(&self, name_or_path: &str) -> Result<Plugin> {
        let path = Path::new(name_or_path);
        if path.is_file() {
            return load_plugin_from_path(path);
        }
        self.plugins
            .iter()
            .find(|p| p.name == name_or_path)
            .cloned()
            .ok_or_else(|| anyhow!("Plugin not found: {name_or_path}"))
    }

    pub fn plugin_path_from_name_or_path(&self, name_or_path: &str) -> Result<PathBuf> {
        let path = Path::new(name_or_path);
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        let candidate = self
            .plugins
            .iter()
            .find(|p| p.name == name_or_path)
            .map(|p| p.path.clone())
            .or_else(|| {
                let by_txt = Self::default_dir()
                    .ok()
                    .map(|dir| dir.join(name_or_path))
                    .filter(|p| p.is_file());
                by_txt
            })
            .ok_or_else(|| anyhow!("Plugin not found: {name_or_path}"))?;
        Ok(candidate)
    }

    pub fn set_enabled(path: &Path, enabled: bool) -> Result<()> {
        let metadata = fs::metadata(path).with_context(|| format!("Failed to stat {:?}", path))?;
        let mut perms = metadata.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut mode = perms.mode();
            if enabled {
                mode |= 0o111;
            } else {
                mode &= !0o111;
            }
            perms.set_mode(mode);
        }
        fs::set_permissions(path, perms).with_context(|| format!("Failed to chmod {:?}", path))?;
        Ok(())
    }

    pub fn create_plugin_stub(name: &str) -> Result<PathBuf> {
        let dir = Self::default_dir()?;
        fs::create_dir_all(&dir).with_context(|| format!("Failed to create {:?}", dir))?;
        let base = name.trim();
        if base.is_empty() {
            return Err(anyhow!("Plugin name cannot be empty"));
        }
        let filename = if base.contains('.') {
            base.to_string()
        } else {
            format!("{base}.sh")
        };
        let path = dir.join(filename);
        if path.exists() {
            return Err(anyhow!("Plugin already exists: {}", path.display()));
        }
        fs::write(
            &path,
            "#!/usr/bin/env bash\n# na:input=json\n# na:output=json\ncat\n",
        )
        .with_context(|| format!("Failed to write {:?}", path))?;
        Self::set_enabled(&path, true)?;
        Ok(path)
    }
}

pub struct PluginRunner {
    plugin: Plugin,
}

impl PluginRunner {
    pub fn new(plugin: Plugin) -> Self {
        Self { plugin }
    }

    pub fn run(&self, actions: &[Action]) -> Result<String> {
        self.run_with_formats(actions, None, None, None)
    }

    pub fn run_with_formats(
        &self,
        actions: &[Action],
        input_override: Option<PluginDataFormat>,
        output_override: Option<PluginDataFormat>,
        divider: Option<&str>,
    ) -> Result<String> {
        let in_fmt = input_override.unwrap_or(self.plugin.input_format);
        let out_fmt = output_override.unwrap_or(self.plugin.output_format);
        let payload = serialize_actions_with_divider(actions, in_fmt, divider)?;
        let mut child = Command::new(&self.plugin.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed launching plugin {}", self.plugin.name))?;

        {
            let stdin = child.stdin.as_mut().context("Plugin stdin unavailable")?;
            use std::io::Write as _;
            stdin.write_all(payload.as_bytes())?;
        }

        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(anyhow!("Plugin {} failed", self.plugin.name));
        }
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        if out_fmt == PluginDataFormat::Json {
            let _ = serde_json::from_str::<serde_json::Value>(&stdout)
                .with_context(|| format!("Plugin {} emitted invalid JSON", self.plugin.name))?;
        }
        Ok(stdout)
    }
}

fn parse_format_hints(path: &Path) -> Result<(PluginDataFormat, PluginDataFormat)> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut input = PluginDataFormat::Json;
    let mut output = PluginDataFormat::Json;

    for line in reader.lines().take(20) {
        let line = line?;
        if let Some(value) =
            parse_hint_value(&line, "na:input=").or_else(|| parse_hint_value(&line, "na-input="))
        {
            input = PluginDataFormat::parse(value).unwrap_or(PluginDataFormat::Json);
        }
        if let Some(value) =
            parse_hint_value(&line, "na:output=").or_else(|| parse_hint_value(&line, "na-output="))
        {
            output = PluginDataFormat::parse(value).unwrap_or(PluginDataFormat::Json);
        }
    }

    Ok((input, output))
}

fn parse_hint_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let lower = line.to_ascii_lowercase();
    let idx = lower.find(key)?;
    let start = idx + key.len();
    let raw = line.get(start..)?.trim();
    let token = raw
        .split(|ch: char| ch.is_whitespace() || ch == ';' || ch == '#')
        .next()
        .unwrap_or("")
        .trim_matches('"')
        .trim_matches('\'');
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_format_hints, parse_hint_value, PluginRegistry};
    use crate::io::xdg::TEST_ENV_MUTEX;
    use crate::plugins::format::PluginDataFormat;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn write_plugin_fixture(contents: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid monotonic time")
            .as_nanos();
        let seq = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!(
            "na_rust_plugin_fixture_{}_{}_{}.sh",
            std::process::id(),
            ts,
            seq
        ));
        fs::write(&path, contents).expect("fixture should write");
        path
    }

    #[test]
    fn parses_na_input_output_hints() {
        let path = write_plugin_fixture(
            r#"#!/usr/bin/env bash
# na:input=yaml
# na:output=csv
echo ok
"#,
        );
        let (input, output) = parse_format_hints(&path).expect("hints should parse");
        fs::remove_file(path).ok();

        assert_eq!(input, PluginDataFormat::Yaml);
        assert_eq!(output, PluginDataFormat::Csv);
    }

    #[test]
    fn default_plugin_format_is_json_without_hints() {
        let path = write_plugin_fixture("#!/usr/bin/env bash\necho ok\n");
        let (input, output) = parse_format_hints(&path).expect("hints should parse");
        fs::remove_file(path).ok();

        assert_eq!(input, PluginDataFormat::Json);
        assert_eq!(output, PluginDataFormat::Json);
    }

    #[test]
    fn parse_hint_value_extracts_token() {
        let value = parse_hint_value("# na-input=text-divider", "na-input=");
        assert_eq!(value, Some("text-divider"));
    }

    #[test]
    fn default_dir_uses_xdg_data_home_when_set() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let xdg = std::env::temp_dir().join("na_rust_plugins_xdg_data_home");
        std::env::set_var("XDG_DATA_HOME", &xdg);
        let path = PluginRegistry::default_dir().expect("default dir should resolve");
        std::env::remove_var("XDG_DATA_HOME");
        assert_eq!(path, xdg.join("na/plugins"));
    }

    #[test]
    fn create_plugin_stub_creates_executable_script() {
        let _env_guard = TEST_ENV_MUTEX.lock().expect("env mutex");
        let xdg = std::env::temp_dir().join(format!(
            "na_rust_plugin_new_fixture_{}_{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::env::set_var("XDG_DATA_HOME", &xdg);
        let path = PluginRegistry::create_plugin_stub("sample").expect("stub should create");
        let body = fs::read_to_string(&path).expect("stub should read");
        std::env::remove_var("XDG_DATA_HOME");
        fs::remove_dir_all(&xdg).ok();

        assert!(body.contains("#!/usr/bin/env bash"));
        assert!(body.contains("na:input=json"));
    }

    #[test]
    fn set_enabled_toggles_executable_bit() {
        let path = write_plugin_fixture("#!/usr/bin/env bash\necho ok\n");
        PluginRegistry::set_enabled(&path, true).expect("enable should work");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode_enabled = fs::metadata(&path).expect("metadata").permissions().mode();
            assert_ne!(mode_enabled & 0o111, 0);
            PluginRegistry::set_enabled(&path, false).expect("disable should work");
            let mode_disabled = fs::metadata(&path).expect("metadata").permissions().mode();
            assert_eq!(mode_disabled & 0o111, 0);
        }
        fs::remove_file(path).ok();
    }
}
