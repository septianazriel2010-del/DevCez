use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

const SKIP_DIRECTORIES: &[&str] = &[".git", ".vercel", "node_modules", "vendor", ".venv"];
const MAX_SCANNED_ENTRIES: usize = 50_000;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLink {
    pub project_id: String,
    pub team_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentProjectInfo {
    pub path: String,
    pub name: String,
    pub framework: String,
    pub desktop_application: bool,
    pub deployable: bool,
    pub validation_errors: Vec<String>,
    pub package_json_present: bool,
    pub composer_json_present: bool,
    pub dependencies_declared: bool,
    pub lockfile_present: bool,
    pub build_command: Option<String>,
    pub output_directory: Option<String>,
    pub install_command: Option<String>,
    pub development_command: Option<String>,
    pub environment_status: String,
    pub environment_files: Vec<String>,
    pub sensitive_files: Vec<String>,
    pub vercel_config_present: bool,
    pub vercel_ignore_present: bool,
    pub vercel_link: Option<ProjectLink>,
    pub cli_installed: bool,
    pub cli_version: Option<String>,
    pub cli_error: Option<String>,
}

pub fn inspect(selected_path: &str) -> Result<DeploymentProjectInfo, String> {
    let path = canonical_project_path(selected_path)?;
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());

    let package = read_json_if_exists(&path.join("package.json"))?;
    let composer = read_json_if_exists(&path.join("composer.json"))?;
    let desktop_application = is_desktop_application(&path, package.as_ref());
    let framework = if desktop_application {
        "Desktop application".to_string()
    } else {
        detect_framework(&path, package.as_ref(), composer.as_ref())
    };
    let package_json_present = package.is_some();
    let composer_json_present = composer.is_some();
    let composer_dependencies_declared = composer
        .as_ref()
        .and_then(|value| value.get("require"))
        .and_then(Value::as_object)
        .map(|dependencies| !dependencies.is_empty())
        .unwrap_or(false);
    let dependencies_declared =
        has_dependencies(package.as_ref()) || composer_dependencies_declared;
    let package_manager = detect_package_manager(&path, package.as_ref());
    let lockfile_present = has_lockfile(&path);
    let scripts = package
        .as_ref()
        .and_then(|value| value.get("scripts"))
        .and_then(Value::as_object);
    let project_settings = read_json_if_exists(&path.join("vercel.json"))?;
    let configured_install_command = project_settings
        .as_ref()
        .and_then(|value| value.get("installCommand"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let script_runner = package_manager.or_else(|| {
        configured_install_command
            .as_deref()
            .and_then(|command| command.split_whitespace().next())
            .filter(|command| matches!(*command, "npm" | "pnpm" | "yarn" | "bun"))
    });
    let build_command = project_settings
        .as_ref()
        .and_then(|value| value.get("buildCommand"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            scripts
                .and_then(|values| values.get("build"))
                .and_then(Value::as_str)
                .and_then(|_| script_runner.map(|runner| format!("{runner} run build")))
        });
    let development_command = project_settings
        .as_ref()
        .and_then(|value| value.get("devCommand"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            scripts
                .and_then(|values| values.get("dev").or_else(|| values.get("start")))
                .and_then(Value::as_str)
                .and_then(|_| {
                    let script_name = if scripts.and_then(|values| values.get("dev")).is_some() {
                        "dev"
                    } else {
                        "start"
                    };
                    script_runner.map(|runner| format!("{runner} run {script_name}"))
                })
        });
    let install_command = configured_install_command.or_else(|| {
        if composer_dependencies_declared
            && has_dependencies(package.as_ref())
            && package_manager.is_some()
        {
            None
        } else {
            detected_install_command(&path, package_manager)
        }
    });
    let angular = read_json_if_exists(&path.join("angular.json"))?;
    let output_directory = project_settings
        .as_ref()
        .and_then(|value| value.get("outputDirectory"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| angular_output_directory(angular.as_ref()))
        .or(configured_output_directory(&path, &framework)?);
    let vercel_config_present = path.join("vercel.json").is_file();
    let angular_config_present = path.join("angular.json").is_file();
    let validation_errors = validate_project(
        &framework,
        package.as_ref(),
        composer.as_ref(),
        package_json_present,
        composer_json_present,
        dependencies_declared,
        build_command.as_deref(),
        install_command.as_deref(),
        vercel_config_present,
        angular_config_present,
    );
    let vercel_link = read_link(&path)?;
    let sensitive_files = scan_sensitive_files(&path)?;
    let environment_files = sensitive_files
        .iter()
        .filter(|filename| {
            Path::new(filename)
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name == ".env" || name.starts_with(".env."))
                .unwrap_or(false)
        })
        .cloned()
        .collect::<Vec<_>>();
    let environment_status = if environment_files.is_empty() {
        "No environment files detected; environment variables were not read.".to_string()
    } else {
        format!(
            "{} environment file(s) detected; contents were not read.",
            environment_files.len()
        )
    };

    let cli = super::engine::cli_status();
    Ok(DeploymentProjectInfo {
        path: path.to_string_lossy().into_owned(),
        name,
        framework,
        desktop_application,
        deployable: validation_errors.is_empty(),
        validation_errors,
        package_json_present,
        composer_json_present,
        dependencies_declared,
        lockfile_present,
        build_command,
        output_directory,
        install_command,
        development_command,
        environment_status,
        environment_files,
        sensitive_files,
        vercel_config_present,
        vercel_ignore_present: path.join(".vercelignore").is_file(),
        vercel_link,
        cli_installed: cli.installed,
        cli_version: cli.version,
        cli_error: cli.error,
    })
}

fn validate_project(
    framework: &str,
    package: Option<&Value>,
    composer: Option<&Value>,
    package_json_present: bool,
    composer_json_present: bool,
    dependencies_declared: bool,
    build_command: Option<&str>,
    install_command: Option<&str>,
    vercel_config_present: bool,
    angular_config_present: bool,
) -> Vec<String> {
    let mut errors = Vec::new();
    if framework == "Desktop application" {
        errors.push(
            "This folder is a desktop application, not a deployable Vercel web project."
                .to_string(),
        );
        return errors;
    }

    let known_framework = matches!(
        framework,
        "Next.js"
            | "Nuxt"
            | "SvelteKit"
            | "Svelte + Vite"
            | "Svelte"
            | "Angular"
            | "Astro"
            | "Remix"
            | "Gatsby"
            | "SolidStart"
            | "Qwik"
            | "Eleventy"
            | "Create React App"
            | "Vue CLI"
            | "Vue + Vite"
            | "Vue"
            | "React + Vite"
            | "React"
            | "Vite"
            | "Static HTML"
    );
    let explicitly_configured = vercel_config_present && build_command.is_some();
    if !known_framework && !explicitly_configured {
        errors.push(
            "Framework is Unknown. Configure a supported Vercel framework, or provide an explicit vercel.json build configuration."
                .to_string(),
        );
    }
    if framework == "Laravel" {
        errors.push(
            "Laravel needs an explicit Vercel-compatible runtime and build configuration; CEZECORE cannot safely infer one."
                .to_string(),
        );
    }

    let static_site = framework == "Static HTML";
    if !static_site && !package_json_present && !composer_json_present {
        errors.push(
            "No supported project manifest was found. Add package.json or configure a supported static web project."
                .to_string(),
        );
    }
    if !static_site && !dependencies_declared {
        errors.push("No application dependencies were declared in the manifest.".to_string());
    }
    if !static_site && install_command.is_none() {
        errors.push(
            "Could not determine an install command. Add a supported package-manager lockfile or an explicit installCommand to vercel.json."
                .to_string(),
        );
    }
    if !static_site && build_command.is_none() {
        errors.push(
            "No build command was found. Add a package.json scripts.build entry or set buildCommand in vercel.json."
                .to_string(),
        );
    }
    if framework == "Angular" && !angular_config_present {
        errors.push("Angular projects require angular.json.".to_string());
    }
    if !static_site && package.is_none() && composer.is_some() && framework != "Laravel" {
        errors.push(
            "Composer-only deployments are not automatically supported by this Vercel workflow."
                .to_string(),
        );
    }
    if package.is_none() && composer.is_none() && !static_site {
        errors.push("A valid package.json or composer.json is required.".to_string());
    }
    if !vercel_config_present && framework == "Unknown" {
        errors.push(
            "A Vercel project configuration is required for an unrecognized framework.".to_string(),
        );
    }
    if package
        .and_then(|value| value.pointer("/engines/node"))
        .and_then(Value::as_str)
        .map(|range| !node_runtime_supported(range))
        .unwrap_or(false)
    {
        errors.push(
            "package.json requires a Node.js version older than the supported Vercel runtime."
                .to_string(),
        );
    }
    if composer
        .and_then(|value| value.pointer("/require/php"))
        .and_then(Value::as_str)
        .map(|range| range.contains("<7") || range.contains("5."))
        .unwrap_or(false)
    {
        errors.push("composer.json requires an unsupported legacy PHP runtime.".to_string());
    }
    errors
}

fn node_runtime_supported(range: &str) -> bool {
    let normalized = range.trim().to_ascii_lowercase();
    !["<18", "^16", "~16", "16.x", "=16", "<=16", "<17"]
        .iter()
        .any(|unsupported| normalized.contains(unsupported))
}

fn is_desktop_application(path: &Path, package: Option<&Value>) -> bool {
    [
        "src-tauri/tauri.conf.json",
        "src-tauri/tauri.conf.json5",
        "tauri.conf.json",
        "electron-builder.yml",
        "electron-builder.yaml",
    ]
    .iter()
    .any(|name| path.join(name).is_file())
        || ["electron", "electron-builder", "electron-forge"]
            .iter()
            .any(|dependency| {
                package
                    .and_then(|value| value.get("dependencies"))
                    .and_then(Value::as_object)
                    .map(|items| items.contains_key(*dependency))
                    .unwrap_or(false)
                    || package
                        .and_then(|value| value.get("devDependencies"))
                        .and_then(Value::as_object)
                        .map(|items| items.contains_key(*dependency))
                        .unwrap_or(false)
            })
}

pub fn link(
    selected_path: &str,
    project_id: &str,
    team_id: Option<&str>,
) -> Result<ProjectLink, String> {
    validate_link_id(project_id)?;
    if let Some(team_id) = team_id.filter(|value| !value.is_empty()) {
        validate_link_id(team_id)?;
    }
    let path = canonical_project_path(selected_path)?;
    let vercel_directory = path.join(".vercel");
    let link_path = vercel_directory.join("project.json");

    match fs::symlink_metadata(&vercel_directory) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err("The existing .vercel directory is a symbolic link; CEZECORE will not write through it.".to_string());
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err("The existing .vercel path is not a directory.".to_string());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Could not inspect the .vercel directory: {error}")),
    }

    match fs::symlink_metadata(&link_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err("The existing .vercel/project.json is a symbolic link; CEZECORE will not follow it.".to_string());
        }
        Ok(_) => {
            let existing = read_link(&path)?.ok_or_else(|| {
                "The existing .vercel/project.json could not be read.".to_string()
            })?;
            if existing.project_id == project_id && existing.team_id.as_deref() == team_id {
                return Ok(existing);
            }
            return Err(
            "This folder is already linked to a different Vercel project. The existing configuration was left unchanged."
                .to_string(),
        );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Could not inspect .vercel/project.json: {error}")),
    }

    fs::create_dir_all(&vercel_directory)
        .map_err(|error| format!("Could not create the Vercel link directory: {error}"))?;
    let link = ProjectLink {
        project_id: project_id.to_string(),
        team_id: team_id.map(str::to_string),
    };
    let mut json = serde_json::Map::new();
    json.insert(
        "projectId".to_string(),
        Value::String(link.project_id.clone()),
    );
    if let Some(team_id) = &link.team_id {
        json.insert("orgId".to_string(), Value::String(team_id.clone()));
    }
    let contents = serde_json::to_vec_pretty(&Value::Object(json))
        .map_err(|error| format!("Could not prepare Vercel project link: {error}"))?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&link_path)
        .map_err(|error| {
            format!(
                "Could not create .vercel/project.json without overwriting existing files: {error}"
            )
        })?;
    use std::io::Write;
    file.write_all(&contents)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("Could not save the Vercel project link: {error}"))?;
    Ok(link)
}

pub fn canonical_project_path(selected_path: &str) -> Result<PathBuf, String> {
    if selected_path.trim().is_empty() {
        return Err("Select a local project folder first.".to_string());
    }
    let path = fs::canonicalize(selected_path)
        .map_err(|error| format!("The selected project folder is unavailable: {error}"))?;
    if !path.is_dir() {
        return Err("The selected project path is not a folder.".to_string());
    }
    Ok(path)
}

fn read_json_if_exists(path: &Path) -> Result<Option<Value>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not inspect {}: {error}", path.display())),
    };
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "CEZECORE will not inspect the symlinked configuration file {}.",
            path.display()
        ));
    }
    if metadata.len() > 5 * 1024 * 1024 {
        return Err(format!(
            "{} is larger than the 5 MiB inspection limit.",
            path.display()
        ));
    }
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) => return Err(format!("Could not read {}: {error}", path.display())),
    };
    let value = serde_json::from_str(&contents)
        .map_err(|error| format!("{} contains invalid JSON: {error}", path.display()))?;
    Ok(Some(value))
}

fn detect_framework(path: &Path, package: Option<&Value>, composer: Option<&Value>) -> String {
    let dependencies = package
        .and_then(|value| value.get("dependencies"))
        .and_then(Value::as_object);
    let dev_dependencies = package
        .and_then(|value| value.get("devDependencies"))
        .and_then(Value::as_object);
    let optional_dependencies = package
        .and_then(|value| value.get("optionalDependencies"))
        .and_then(Value::as_object);
    let has_package = |name: &str| {
        dependencies
            .map(|items| items.contains_key(name))
            .unwrap_or(false)
            || dev_dependencies
                .map(|items| items.contains_key(name))
                .unwrap_or(false)
            || optional_dependencies
                .map(|items| items.contains_key(name))
                .unwrap_or(false)
    };
    let composer_requires_laravel = composer
        .and_then(|value| value.get("require"))
        .and_then(Value::as_object)
        .map(|items| items.contains_key("laravel/framework"))
        .unwrap_or(false);

    if composer_requires_laravel && path.join("artisan").is_file() {
        "Laravel".to_string()
    } else if has_package("next") {
        "Next.js".to_string()
    } else if has_package("@sveltejs/kit") {
        "SvelteKit".to_string()
    } else if has_package("nuxt") || has_package("@nuxt/kit") {
        "Nuxt".to_string()
    } else if has_package("@angular/core") {
        "Angular".to_string()
    } else if has_package("astro") {
        "Astro".to_string()
    } else if has_package("@remix-run/react") || has_package("@remix-run/node") {
        "Remix".to_string()
    } else if has_package("gatsby") {
        "Gatsby".to_string()
    } else if has_package("@solidjs/start") || has_package("solid-start") {
        "SolidStart".to_string()
    } else if has_package("@builder.io/qwik") || has_package("@builder.io/qwik-city") {
        "Qwik".to_string()
    } else if has_package("@11ty/eleventy") {
        "Eleventy".to_string()
    } else if has_package("react-scripts") {
        "Create React App".to_string()
    } else if has_package("@vue/cli-service") {
        "Vue CLI".to_string()
    } else if has_package("svelte") && has_package("vite") {
        "Svelte + Vite".to_string()
    } else if has_package("vue") && has_package("vite") {
        "Vue + Vite".to_string()
    } else if has_package("vue") {
        "Vue".to_string()
    } else if has_package("react") && has_package("vite") {
        "React + Vite".to_string()
    } else if has_package("react") {
        "React".to_string()
    } else if has_package("svelte") {
        "Svelte".to_string()
    } else if has_package("vite") {
        "Vite".to_string()
    } else if path.join("index.html").is_file() {
        "Static HTML".to_string()
    } else {
        "Unknown".to_string()
    }
}

fn has_dependencies(package: Option<&Value>) -> bool {
    ["dependencies", "devDependencies", "optionalDependencies"]
        .iter()
        .any(|field| {
            package
                .and_then(|value| value.get(*field))
                .and_then(Value::as_object)
                .map(|dependencies| !dependencies.is_empty())
                .unwrap_or(false)
        })
}

fn detect_package_manager(path: &Path, package: Option<&Value>) -> Option<&'static str> {
    let declared = package
        .and_then(|value| value.get("packageManager"))
        .and_then(Value::as_str)
        .and_then(|value| {
            ["pnpm", "yarn", "bun", "npm"]
                .into_iter()
                .find(|manager| value.starts_with(&format!("{manager}@")))
        });
    if declared.is_some() {
        return declared;
    }

    let detected = [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
        ("package-lock.json", "npm"),
        ("npm-shrinkwrap.json", "npm"),
    ]
    .into_iter()
    .filter(|(file, _)| path.join(file).is_file())
    .map(|(_, manager)| manager)
    .collect::<std::collections::HashSet<_>>();
    if detected.len() == 1 {
        detected.into_iter().next()
    } else {
        None
    }
}

fn has_lockfile(path: &Path) -> bool {
    [
        "pnpm-lock.yaml",
        "yarn.lock",
        "bun.lock",
        "bun.lockb",
        "package-lock.json",
        "npm-shrinkwrap.json",
        "composer.lock",
    ]
    .iter()
    .any(|file| path.join(file).is_file())
}

fn detected_install_command(path: &Path, package_manager: Option<&str>) -> Option<String> {
    match package_manager {
        Some("npm")
            if path.join("package-lock.json").is_file()
                || path.join("npm-shrinkwrap.json").is_file() =>
        {
            Some("npm ci".to_string())
        }
        Some("npm") => Some("npm install".to_string()),
        Some("pnpm") => Some("pnpm install".to_string()),
        Some("yarn") => Some("yarn install".to_string()),
        Some("bun") => Some("bun install".to_string()),
        None if path.join("composer.lock").is_file() => Some("composer install".to_string()),
        _ => None,
    }
}

fn configured_output_directory(path: &Path, framework: &str) -> Result<Option<String>, String> {
    if framework == "Static HTML" {
        return Ok(Some(".".to_string()));
    }
    if framework.contains("Vite") || framework == "Vite" {
        let configured = vite_output_directory(path)?;
        return Ok(configured.or_else(|| (!has_vite_config(path)).then(|| "dist".to_string())));
    }
    if framework == "Create React App" {
        return Ok(Some("build".to_string()));
    }
    if framework == "Vue CLI" {
        let configured = vue_cli_output_directory(path)?;
        return Ok(configured
            .or_else(|| (!path.join("vue.config.js").is_file()).then(|| "dist".to_string())));
    }
    Ok(None)
}

pub fn vercel_framework(framework: &str) -> Option<&'static str> {
    match framework {
        "Next.js" => Some("nextjs"),
        "Nuxt" => Some("nuxtjs"),
        "SvelteKit" => Some("sveltekit"),
        "Svelte + Vite" | "Svelte" => Some("svelte"),
        "Angular" => Some("angular"),
        "Astro" => Some("astro"),
        "Remix" => Some("remix"),
        "Gatsby" => Some("gatsby"),
        "SolidStart" => Some("solidstart"),
        "Eleventy" => Some("eleventy"),
        "Create React App" => Some("create-react-app"),
        "Vue CLI" | "Vue" => Some("vue"),
        "Vue + Vite" | "React + Vite" | "Vite" => Some("vite"),
        _ => None,
    }
}

pub fn normalize_project_name(name: &str) -> String {
    let mut normalized = String::new();
    for character in name.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            normalized.push(character);
        } else if !normalized.is_empty() && !normalized.ends_with('-') {
            normalized.push('-');
        }
    }

    normalized.truncate(100);
    let normalized = normalized.trim_matches(|character| matches!(character, '-' | '.' | '_'));
    if normalized.is_empty() {
        "cezecore-project".to_string()
    } else {
        normalized.to_string()
    }
}

pub fn validate_project_name(name: &str) -> Result<(), String> {
    let bytes = name.as_bytes();
    let is_alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    let is_separator = |byte: u8| matches!(byte, b'-' | b'.' | b'_');
    let valid = !bytes.is_empty()
        && bytes.len() <= 100
        && is_alphanumeric(bytes[0])
        && is_alphanumeric(*bytes.last().unwrap_or(&0))
        && bytes
            .iter()
            .all(|byte| is_alphanumeric(*byte) || is_separator(*byte))
        && !bytes.windows(2).any(|pair| pair == b".." || pair == b"__")
        && !name.contains("---");
    if valid {
        Ok(())
    } else {
        Err("Vercel project name is invalid. It must be 1-100 lowercase letters, numbers, dots, underscores, or hyphens; begin and end with a letter or number; and contain no triple hyphen or repeated dots/underscores.".to_string())
    }
}

fn has_vite_config(path: &Path) -> bool {
    [
        "vite.config.js",
        "vite.config.mjs",
        "vite.config.cjs",
        "vite.config.ts",
        "vite.config.mts",
        "vite.config.cts",
    ]
    .iter()
    .any(|name| path.join(name).is_file())
}

fn vite_output_directory(path: &Path) -> Result<Option<String>, String> {
    for name in [
        "vite.config.js",
        "vite.config.mjs",
        "vite.config.cjs",
        "vite.config.ts",
        "vite.config.mts",
        "vite.config.cts",
    ] {
        if let Some(config) = read_config_text(&path.join(name))? {
            return Ok(configured_string_property(&config, "outDir"));
        }
    }
    Ok(None)
}

fn vue_cli_output_directory(path: &Path) -> Result<Option<String>, String> {
    let Some(config) = read_config_text(&path.join("vue.config.js"))? else {
        return Ok(None);
    };
    Ok(configured_string_property(&config, "outputDir"))
}

fn read_config_text(path: &Path) -> Result<Option<String>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not inspect {}: {error}", path.display())),
    };
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "CEZECORE will not inspect the symlinked configuration file {}.",
            path.display()
        ));
    }
    if metadata.len() > 5 * 1024 * 1024 {
        return Err(format!(
            "{} is larger than the 5 MiB inspection limit.",
            path.display()
        ));
    }
    fs::read_to_string(path)
        .map(Some)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))
}

fn configured_string_property(contents: &str, property: &str) -> Option<String> {
    for (property_at, _) in contents.match_indices(property) {
        let preceding = contents[..property_at].chars().next_back();
        if preceding
            .map(|character| {
                character.is_ascii_alphanumeric() || character == '_' || character == '$'
            })
            .unwrap_or(false)
        {
            continue;
        }
        let after_property = contents[property_at + property.len()..].trim_start();
        let Some(value) = after_property.strip_prefix(':') else {
            continue;
        };
        let value = value.trim_start();
        let Some(quote) = value.chars().next() else {
            continue;
        };
        if quote != '\'' && quote != '"' && quote != '`' {
            continue;
        }
        let remainder = &value[quote.len_utf8()..];
        let Some(end) = remainder.find(quote) else {
            continue;
        };
        let configured = remainder[..end].trim();
        if !configured.is_empty() {
            return Some(configured.to_string());
        }
    }
    None
}

fn angular_output_directory(config: Option<&Value>) -> Option<String> {
    config?
        .get("projects")?
        .as_object()?
        .values()
        .find_map(|project| {
            project
                .pointer("/architect/build/options/outputPath")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    project
                        .pointer("/targets/build/options/outputPath")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
        })
}

fn read_link(path: &Path) -> Result<Option<ProjectLink>, String> {
    let vercel_directory = path.join(".vercel");
    match fs::symlink_metadata(&vercel_directory) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(
                "The existing .vercel directory is a symbolic link; CEZECORE will not follow it."
                    .to_string(),
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Could not inspect the Vercel link directory: {error}"
            ))
        }
        _ => {}
    }
    let link_path = path.join(".vercel").join("project.json");
    match fs::symlink_metadata(&link_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err("The existing .vercel/project.json is a symbolic link; CEZECORE will not follow it.".to_string());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Could not inspect the Vercel project link: {error}"
            ))
        }
        _ => {}
    }
    match fs::read_to_string(&link_path) {
        Ok(contents) => {
            let value: Value = serde_json::from_str(&contents)
                .map_err(|error| format!("The existing Vercel project link is invalid: {error}"))?;
            let project_id = value
                .get("projectId")
                .and_then(Value::as_str)
                .ok_or_else(|| "The existing Vercel project link has no projectId.".to_string())?;
            Ok(Some(ProjectLink {
                project_id: project_id.to_string(),
                team_id: value
                    .get("orgId")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            }))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Could not read the Vercel project link: {error}")),
    }
}

fn scan_sensitive_files(root: &Path) -> Result<Vec<String>, String> {
    let mut pending = vec![root.to_path_buf()];
    let mut matches = Vec::new();
    let mut scanned = 0usize;
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|error| {
            format!(
                "Could not inspect deployment files in {}: {error}",
                directory.display()
            )
        })?;
        for entry in entries {
            scanned += 1;
            if scanned > MAX_SCANNED_ENTRIES {
                return Err("Project security scan exceeded 50,000 entries. Narrow the selected folder and retry.".to_string());
            }
            let entry =
                entry.map_err(|error| format!("Could not inspect a deployment file: {error}"))?;
            let file_type = entry.file_type().map_err(|error| {
                format!("Could not inspect {}: {error}", entry.path().display())
            })?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if file_type.is_dir() {
                if !SKIP_DIRECTORIES
                    .iter()
                    .any(|skip| name.eq_ignore_ascii_case(skip))
                {
                    pending.push(entry.path());
                }
            } else if file_type.is_symlink() || (file_type.is_file() && looks_sensitive(&name)) {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|_| {
                        "Could not safely determine a project-relative file path.".to_string()
                    })?
                    .to_string_lossy()
                    .into_owned();
                matches.push(relative);
            }
        }
    }
    matches.sort();
    Ok(matches)
}

fn looks_sensitive(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == ".env"
        || lower.starts_with(".env.")
        || lower == ".npmrc"
        || lower == "credentials"
        || lower == "credentials.json"
        || lower == "id_rsa"
        || lower == "id_ed25519"
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with(".p12")
        || lower.ends_with(".pfx")
        || lower.ends_with(".tfstate")
}

fn validate_link_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 200
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("The selected Vercel project identifier is invalid.".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_project_path, detect_framework, inspect, link, looks_sensitive,
        normalize_project_name, validate_project_name, vercel_framework,
    };
    use serde_json::json;
    use std::{fs, path::Path};

    #[test]
    fn recognizes_supported_frameworks_from_manifest_and_files() {
        let next = json!({"dependencies": {"next": "15"}});
        assert_eq!(
            detect_framework(Path::new("."), Some(&next), None),
            "Next.js"
        );
        let vite = json!({"devDependencies": {"react": "18", "vite": "6"}});
        assert_eq!(
            detect_framework(Path::new("."), Some(&vite), None),
            "React + Vite"
        );
        assert_eq!(
            detect_framework(
                Path::new("."),
                Some(&json!({"dependencies": {"@sveltejs/kit": "2"}})),
                None
            ),
            "SvelteKit"
        );
        assert_eq!(
            detect_framework(
                Path::new("."),
                Some(&json!({"dependencies": {"astro": "5"}})),
                None
            ),
            "Astro"
        );
        for (dependency, framework) in [
            ("vue", "Vue"),
            ("nuxt", "Nuxt"),
            ("svelte", "Svelte"),
            ("@angular/core", "Angular"),
            ("@remix-run/react", "Remix"),
            ("gatsby", "Gatsby"),
        ] {
            let mut dependencies = serde_json::Map::new();
            dependencies.insert(dependency.to_string(), json!("1"));
            let manifest = json!({"dependencies": dependencies});
            assert_eq!(
                detect_framework(Path::new("."), Some(&manifest), None),
                framework
            );
        }
        assert_eq!(detect_framework(Path::new("."), None, None), "Unknown");
    }

    #[test]
    fn detects_commands_and_output_from_project_configuration() {
        let path = temporary_project("configured");
        fs::write(
            path.join("package.json"),
            r#"{"packageManager":"pnpm@9.0.0","scripts":{"build":"vite build"},"dependencies":{"react":"18","vite":"6"}}"#,
        )
        .unwrap();
        fs::write(path.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
        fs::write(
            path.join("vite.config.ts"),
            r#"export default { build: { outDir : "release bundle" } };"#,
        )
        .unwrap();
        let info = inspect(path.to_str().unwrap()).unwrap();
        assert_eq!(info.framework, "React + Vite");
        assert_eq!(info.build_command.as_deref(), Some("pnpm run build"));
        assert_eq!(info.install_command.as_deref(), Some("pnpm install"));
        assert_eq!(info.output_directory.as_deref(), Some("release bundle"));
        assert!(info.dependencies_declared);
        assert!(info.lockfile_present);
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn does_not_invent_missing_commands_or_output_configuration() {
        let path = temporary_project("incomplete");
        fs::write(
            path.join("package.json"),
            r#"{"dependencies":{"next":"15"},"scripts":{"dev":"next dev"}}"#,
        )
        .unwrap();
        let info = inspect(path.to_str().unwrap()).unwrap();
        assert_eq!(info.framework, "Next.js");
        assert_eq!(info.build_command, None);
        assert_eq!(info.install_command, None);
        assert_eq!(info.output_directory, None);
        assert!(!info.lockfile_present);
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn static_web_projects_pass_and_desktop_applications_are_blocked() {
        let static_path = temporary_project("validated-static");
        fs::write(static_path.join("index.html"), "<h1>Static page</h1>").unwrap();
        let static_info = inspect(static_path.to_str().unwrap()).unwrap();
        assert!(static_info.deployable);
        assert!(static_info.validation_errors.is_empty());

        let desktop_path = temporary_project("desktop");
        fs::create_dir_all(desktop_path.join("src-tauri")).unwrap();
        fs::write(
            desktop_path.join("package.json"),
            r#"{"scripts":{"build":"vite build"},"dependencies":{"react":"18","vite":"6"},"packageManager":"npm@10"}"#,
        )
        .unwrap();
        fs::write(desktop_path.join("src-tauri").join("tauri.conf.json"), "{}").unwrap();
        fs::write(desktop_path.join("package-lock.json"), "{}").unwrap();
        let desktop_info = inspect(desktop_path.to_str().unwrap()).unwrap();
        assert_eq!(desktop_info.framework, "Desktop application");
        assert!(desktop_info.desktop_application);
        assert!(!desktop_info.deployable);
        assert!(desktop_info
            .validation_errors
            .iter()
            .any(|error| error.contains("desktop application")));
        let _ = fs::remove_dir_all(static_path);
        let _ = fs::remove_dir_all(desktop_path);
    }

    #[test]
    fn framework_mapping_uses_vercel_identifiers_only_when_known() {
        assert_eq!(vercel_framework("Next.js"), Some("nextjs"));
        assert_eq!(vercel_framework("React + Vite"), Some("vite"));
        assert_eq!(vercel_framework("Static HTML"), None);
        assert_eq!(vercel_framework("Unknown"), None);
    }

    #[test]
    fn normalizes_and_validates_vercel_project_names() {
        assert_eq!(normalize_project_name("Project-Kel-9"), "project-kel-9");
        assert_eq!(
            normalize_project_name("  My Local PROJECT  "),
            "my-local-project"
        );
        assert_eq!(normalize_project_name(" --A...B___C---! "), "a-b-c");

        let long_name = normalize_project_name(&"A".repeat(150));
        assert_eq!(long_name.len(), 100);
        assert!(validate_project_name(&long_name).is_ok());

        assert_eq!(normalize_project_name("!@#$%^&*()"), "cezecore-project");
        assert!(validate_project_name("project-kel-9").is_ok());
        assert!(validate_project_name("project--kel").is_ok());
        assert!(validate_project_name("project---kel").is_err());
        assert!(validate_project_name("project..kel").is_err());
        assert!(validate_project_name("Project-Kel-9").is_err());
        assert!(validate_project_name(&"a".repeat(101)).is_err());
    }

    #[test]
    fn detects_sensitive_file_names_without_reading_contents() {
        assert!(looks_sensitive(".env.production"));
        assert!(looks_sensitive("private-key.pem"));
        assert!(!looks_sensitive("index.html"));
    }

    #[test]
    fn inspects_static_html_and_laravel_projects_and_rejects_missing_paths() {
        let static_path = temporary_project("static");
        fs::write(static_path.join("index.html"), "<h1>Local page</h1>").unwrap();
        assert_eq!(
            inspect(static_path.to_str().unwrap()).unwrap().framework,
            "Static HTML"
        );

        let laravel_path = temporary_project("laravel");
        fs::write(laravel_path.join("artisan"), "").unwrap();
        fs::write(
            laravel_path.join("composer.json"),
            r#"{"require":{"laravel/framework":"^11.0"}}"#,
        )
        .unwrap();
        assert_eq!(
            inspect(laravel_path.to_str().unwrap()).unwrap().framework,
            "Laravel"
        );

        assert!(canonical_project_path(static_path.join("missing").to_str().unwrap()).is_err());
        let _ = fs::remove_dir_all(static_path);
        let _ = fs::remove_dir_all(laravel_path);
    }

    #[test]
    fn linking_is_explicit_and_never_replaces_an_existing_project_link() {
        let path = temporary_project("link");
        let saved = link(path.to_str().unwrap(), "prj_first", None).unwrap();
        assert_eq!(saved.project_id, "prj_first");
        assert!(link(path.to_str().unwrap(), "prj_second", None).is_err());
        let contents = fs::read_to_string(path.join(".vercel").join("project.json")).unwrap();
        assert!(contents.contains("prj_first"));
        assert!(!contents.contains("prj_second"));
        let _ = fs::remove_dir_all(path);
    }

    fn temporary_project(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "cezecore-deployment-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
