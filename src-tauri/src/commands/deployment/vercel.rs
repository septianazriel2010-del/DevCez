use super::project;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const API_BASE: &str = "https://api.vercel.com";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VercelAccount {
    pub id: String,
    pub username: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VercelProject {
    pub id: String,
    pub name: String,
    pub framework: Option<String>,
    pub team_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentRecord {
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub url: Option<String>,
    pub state: String,
    pub target: String,
    pub source: Option<String>,
    pub branch: Option<String>,
    pub commit_sha: Option<String>,
    pub commit_message: Option<String>,
    pub created_at: i64,
    pub ready_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentLog {
    pub timestamp: Option<i64>,
    pub kind: String,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct CreateProjectOptions<'a> {
    pub name: &'a str,
    pub framework: Option<&'a str>,
    pub build_command: Option<&'a str>,
    pub install_command: Option<&'a str>,
    pub output_directory: Option<&'a str>,
    pub development_command: Option<&'a str>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectResponse {
    id: String,
    name: String,
    framework: Option<String>,
    team_id: Option<String>,
}

pub async fn get_account(token: &str) -> Result<VercelAccount, String> {
    let response = get_json(token, "/v2/user", &[]).await?;
    let user = response.get("user").unwrap_or(&response);
    let id = string_field(user, &["uid", "id"]).ok_or_else(|| {
        "Vercel authenticated the request but returned no account identifier.".to_string()
    })?;
    Ok(VercelAccount {
        id,
        username: string_field(user, &["username", "name"]),
        email: string_field(user, &["email"]),
        name: string_field(user, &["name", "username"]),
    })
}

pub async fn list_projects(token: &str) -> Result<Vec<VercelProject>, String> {
    let response = get_json(token, "/v9/projects", &[("limit", "100")]).await?;
    let projects = response
        .get("projects")
        .and_then(Value::as_array)
        .ok_or_else(|| "Vercel returned an unexpected projects response.".to_string())?;
    Ok(projects
        .iter()
        .filter_map(|project| {
            Some(VercelProject {
                id: string_field(project, &["id", "projectId"])?,
                name: string_field(project, &["name"])?,
                framework: string_field(project, &["framework"]),
                team_id: string_field(project, &["teamId"]),
            })
        })
        .collect())
}

pub async fn get_project(
    token: &str,
    project_id_or_name: &str,
    team_id: Option<&str>,
) -> Result<Option<VercelProject>, String> {
    validate_api_identifier(project_id_or_name)?;
    let path = format!("/v9/projects/{project_id_or_name}");
    let mut query = Vec::new();
    if let Some(team_id) = team_id.filter(|value| !value.is_empty()) {
        validate_api_identifier(team_id)?;
        query.push(("teamId", team_id));
    }
    let response = match request(token, reqwest::Method::GET, &path, &query, None).await {
        Ok(response) => response,
        Err((status, body)) => {
            if status == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            return Err(api_error(token, status, &body));
        }
    };
    let project: ProjectResponse = response
        .json()
        .await
        .map_err(|error| format!("Vercel returned an invalid project response: {error}"))?;
    Ok(Some(VercelProject {
        id: project.id,
        name: project.name,
        framework: project.framework,
        team_id: team_id.map(str::to_string).or(project.team_id),
    }))
}

pub async fn create_project(
    token: &str,
    options: CreateProjectOptions<'_>,
) -> Result<VercelProject, String> {
    project::validate_project_name(options.name)
        .map_err(|error| format!("Cannot create Vercel project: {error}"))?;
    let mut body = serde_json::Map::new();
    body.insert("name".to_string(), Value::String(options.name.to_string()));
    body.insert("skipGitConnectDuringLink".to_string(), Value::Bool(true));
    for (field, value) in [
        ("framework", options.framework),
        ("buildCommand", options.build_command),
        ("installCommand", options.install_command),
        ("outputDirectory", options.output_directory),
        ("devCommand", options.development_command),
    ] {
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            body.insert(field.to_string(), Value::String(value.to_string()));
        }
    }
    let response = request(
        token,
        reqwest::Method::POST,
        "/v11/projects",
        &[],
        Some(&Value::Object(body)),
    )
    .await
    .map_err(|(status, body)| api_error(token, status, &body))?;
    let project: ProjectResponse = response
        .json()
        .await
        .map_err(|error| format!("Vercel returned an invalid created-project response: {error}"))?;
    project::validate_project_name(&project.name)
        .map_err(|error| format!("Vercel returned a project with an invalid name: {error}"))?;
    Ok(VercelProject {
        id: project.id,
        name: project.name,
        framework: project.framework,
        team_id: project.team_id,
    })
}

pub async fn list_deployments(
    token: &str,
    project_id: &str,
    team_id: Option<&str>,
) -> Result<Vec<DeploymentRecord>, String> {
    validate_api_identifier(project_id)?;
    let mut query = vec![("projectId", project_id), ("limit", "25")];
    if let Some(team_id) = team_id.filter(|value| !value.is_empty()) {
        validate_api_identifier(team_id)?;
        query.push(("teamId", team_id));
    }
    let response = get_json(token, "/v7/deployments", &query).await?;
    let deployments = response
        .get("deployments")
        .and_then(Value::as_array)
        .ok_or_else(|| "Vercel returned an unexpected deployments response.".to_string())?;

    Ok(deployments
        .iter()
        .filter_map(|deployment| {
            let id = string_field(deployment, &["uid", "id", "deploymentId"])?;
            let project_id = string_field(deployment, &["projectId"])?;
            let project_name =
                string_field(deployment, &["name"]).unwrap_or_else(|| project_id.clone());
            let raw_url = string_field(deployment, &["url"]);
            let url = raw_url.map(|value| {
                if value.starts_with("https://") {
                    value
                } else {
                    format!("https://{value}")
                }
            });
            let meta = deployment.get("meta").unwrap_or(&Value::Null);
            let git_source = deployment.get("gitSource").unwrap_or(&Value::Null);
            let source = string_field(deployment, &["source"])
                .or_else(|| string_field(git_source, &["type"]))
                .or_else(|| string_field(meta, &["gitProvider"]).map(|_| "git".to_string()));
            let branch = string_field(git_source, &["ref"]).or_else(|| {
                string_field(
                    meta,
                    &["githubCommitRef", "gitlabCommitRef", "bitbucketCommitRef"],
                )
            });
            let commit_sha = string_field(git_source, &["sha"]).or_else(|| {
                string_field(
                    meta,
                    &["githubCommitSha", "gitlabCommitSha", "bitbucketCommitSha"],
                )
            });
            let commit_message = string_field(
                meta,
                &[
                    "githubCommitMessage",
                    "gitlabCommitMessage",
                    "bitbucketCommitMessage",
                ],
            );
            Some(DeploymentRecord {
                id,
                project_id,
                project_name,
                url,
                state: display_state(
                    string_field(deployment, &["readyState", "state"])
                        .as_deref()
                        .unwrap_or("UNKNOWN"),
                ),
                target: if string_field(deployment, &["target"]).as_deref() == Some("production") {
                    "Production".to_string()
                } else {
                    "Preview".to_string()
                },
                source,
                branch,
                commit_sha,
                commit_message,
                created_at: number_field(deployment, &["createdAt", "created"]).unwrap_or_default(),
                ready_at: number_field(deployment, &["ready", "readyAt", "buildingAt"]),
            })
        })
        .collect())
}

pub async fn get_deployment_logs(
    token: &str,
    deployment_id: &str,
    team_id: Option<&str>,
) -> Result<Vec<DeploymentLog>, String> {
    validate_api_identifier(deployment_id)?;
    let path = format!("/v3/deployments/{deployment_id}/events");
    let mut query = vec![("limit", "200"), ("direction", "backward")];
    if let Some(team_id) = team_id.filter(|value| !value.is_empty()) {
        validate_api_identifier(team_id)?;
        query.push(("teamId", team_id));
    }
    let events = get_json(token, &path, &query).await?;
    let events = events
        .as_array()
        .ok_or_else(|| "Vercel returned an unexpected deployment logs response.".to_string())?;

    Ok(events
        .iter()
        .filter_map(|event| {
            let payload = event.get("payload").unwrap_or(&Value::Null);
            let message = string_field(event, &["text"])
                .or_else(|| string_field(payload, &["text"]))
                .or_else(|| string_field(event, &["type"]))?;
            Some(DeploymentLog {
                timestamp: number_field(event, &["created", "date"]),
                kind: string_field(event, &["type"]).unwrap_or_else(|| "log".to_string()),
                message: redact(&message, token),
            })
        })
        .collect())
}

async fn get_json(token: &str, path: &str, query: &[(&str, &str)]) -> Result<Value, String> {
    let response = request(token, reqwest::Method::GET, path, query, None)
        .await
        .map_err(|(status, body)| api_error(token, status, &body))?;
    response
        .json()
        .await
        .map_err(|error| format!("Vercel returned invalid JSON: {error}"))
}

async fn request(
    token: &str,
    method: reqwest::Method,
    path: &str,
    query: &[(&str, &str)],
    body: Option<&Value>,
) -> Result<reqwest::Response, (reqwest::StatusCode, String)> {
    let mut url = reqwest::Url::parse(API_BASE).map_err(|_| {
        (
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "Vercel API URL is unavailable.".to_string(),
        )
    })?;
    {
        let mut segments = url.path_segments_mut().map_err(|_| {
            (
                reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                "Vercel API URL is unavailable.".to_string(),
            )
        })?;
        segments.pop_if_empty();
        for segment in path.trim_start_matches('/').split('/') {
            segments.push(segment);
        }
    }
    url.query_pairs_mut().extend_pairs(query.iter().copied());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(25))
        .build()
        .map_err(|error| {
            (
                reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Could not create a Vercel API request: {error}"),
            )
        })?;
    let mut request = client
        .request(method, url)
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "application/json");
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request.send().await.map_err(|error| {
        (
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            format!("Could not reach the Vercel API: {error}"),
        )
    })?;

    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response
        .text()
        .await
        .unwrap_or_else(|error| format!("Could not read the Vercel API response: {error}"));
    Err((status, body))
}

fn api_error(token: &str, status: reqwest::StatusCode, body: &str) -> String {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return "Vercel rejected the API token. It may be invalid, expired, or revoked."
            .to_string();
    }
    if status == reqwest::StatusCode::FORBIDDEN {
        return "Vercel denied this request. Check the token's account and project permissions."
            .to_string();
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return "Vercel API rate limit reached. Wait before refreshing again.".to_string();
    }
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            let detail = body.trim();
            if detail.is_empty() {
                format!("HTTP {status}")
            } else {
                detail.to_string()
            }
        });
    format!("Vercel API request failed: {}", redact(&detail, token))
}

fn validate_api_identifier(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 200
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("The Vercel identifier is invalid.".to_string());
    }
    Ok(())
}

fn string_field(value: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn number_field(value: &Value, names: &[&str]) -> Option<i64> {
    names.iter().find_map(|name| {
        let value = value.get(*name)?;
        value
            .as_i64()
            .or_else(|| value.as_str().and_then(|string| string.parse().ok()))
    })
}

fn display_state(state: &str) -> String {
    match state.to_ascii_uppercase().as_str() {
        "INITIALIZING" | "QUEUED" | "BLOCKED" => "Queued".to_string(),
        "BUILDING" => "Building".to_string(),
        "READY" => "Ready".to_string(),
        "ERROR" => "Error".to_string(),
        "CANCELED" | "CANCELLED" => "Cancelled".to_string(),
        other => other.to_string(),
    }
}

fn redact(value: &str, token: &str) -> String {
    if token.is_empty() {
        value.to_string()
    } else {
        value.replace(token, "[redacted]")
    }
}

#[cfg(test)]
mod tests {
    use super::{display_state, validate_api_identifier};

    #[test]
    fn maps_vercel_deployment_states_without_inventing_progress() {
        assert_eq!(display_state("INITIALIZING"), "Queued");
        assert_eq!(display_state("BUILDING"), "Building");
        assert_eq!(display_state("READY"), "Ready");
        assert_eq!(display_state("ERROR"), "Error");
        assert_eq!(display_state("CANCELED"), "Cancelled");
        assert_eq!(display_state("PAUSED"), "PAUSED");
    }

    #[test]
    fn deployment_identifiers_cannot_inject_api_path_segments() {
        assert!(validate_api_identifier("dpl_abc-123").is_ok());
        assert!(validate_api_identifier("https://attacker.example").is_err());
        assert!(validate_api_identifier("../other").is_err());
    }
}
