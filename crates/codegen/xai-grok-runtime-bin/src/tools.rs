use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::time::timeout;
use xai_grok_sampling_types::{ToolCall, ToolSpec};
use xai_tty_utils::ProcessScope;

pub(crate) const WEB_SEARCH_TOOL_NAME: &str = "web_search_runtime";

pub(crate) struct ToolRuntime {
    root: PathBuf,
    allow_write: bool,
    allow_shell: bool,
    timeout: Duration,
    max_output_bytes: usize,
    process_scope: ProcessScope,
}

impl ToolRuntime {
    pub(crate) fn new(
        root: PathBuf,
        allow_write: bool,
        allow_shell: bool,
        timeout_secs: u64,
        max_output_bytes: usize,
    ) -> Self {
        Self {
            root,
            allow_write,
            allow_shell,
            timeout: Duration::from_secs(timeout_secs),
            max_output_bytes,
            process_scope: ProcessScope::new(),
        }
    }

    pub(crate) fn specs(&self) -> Vec<ToolSpec> {
        let mut specs = vec![
            spec(
                WEB_SEARCH_TOOL_NAME,
                "Search the public web for current information. Use this for weather, news, recent facts, or whenever fresh sources are needed.",
                json!({
                    "type": "object",
                    "properties": { "query": { "type": "string" } },
                    "required": ["query"],
                    "additionalProperties": false
                }),
            ),
            spec(
                "read_file",
                "Read a UTF-8 text file inside the workspace with line numbers.",
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "offset": { "type": "integer", "minimum": 1 },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 2000 }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }),
            ),
            spec(
                "list_files",
                "List files under a workspace directory using ripgrep's ignore rules.",
                json!({
                    "type": "object",
                    "properties": { "path": { "type": "string", "default": "." } },
                    "additionalProperties": false
                }),
            ),
            spec(
                "search",
                "Search file contents inside the workspace with a regular expression.",
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "path": { "type": "string", "default": "." }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }),
            ),
        ];
        if self.allow_write {
            specs.extend([
                spec(
                    "write_file",
                    "Write complete UTF-8 content to a file inside the workspace.",
                    json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "content": { "type": "string" }
                        },
                        "required": ["path", "content"],
                        "additionalProperties": false
                    }),
                ),
                spec(
                    "edit_file",
                    "Replace one exact text occurrence in a workspace file. Set replace_all only when every occurrence should change.",
                    json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "old_text": { "type": "string" },
                            "new_text": { "type": "string" },
                            "replace_all": { "type": "boolean", "default": false }
                        },
                        "required": ["path", "old_text", "new_text"],
                        "additionalProperties": false
                    }),
                ),
            ]);
        }
        if self.allow_shell {
            specs.push(spec(
                "bash",
                "Run a shell command in the workspace. Returns exit code, stdout, and stderr.",
                json!({
                    "type": "object",
                    "properties": { "command": { "type": "string" } },
                    "required": ["command"],
                    "additionalProperties": false
                }),
            ));
        }
        specs
    }

    pub(crate) async fn execute(&self, call: &ToolCall) -> String {
        let result = self.execute_inner(call).await;
        self.format_result(result)
    }

    pub(crate) fn format_result(&self, result: Result<String>) -> String {
        match result {
            Ok(output) => truncate(&output, self.max_output_bytes),
            Err(error) => truncate(&format!("Error: {error:#}"), self.max_output_bytes),
        }
    }

    pub(crate) fn web_search_query(&self, call: &ToolCall) -> Result<String> {
        let args: WebSearchArgs = parse(&call.arguments)?;
        let query = args.query.trim();
        if query.is_empty() {
            bail!("web search query must not be empty");
        }
        Ok(query.to_owned())
    }

    pub(crate) fn describe_call(&self, call: &ToolCall) -> String {
        match call.name.as_str() {
            WEB_SEARCH_TOOL_NAME => parse::<WebSearchArgs>(&call.arguments)
                .map(|args| format!("query={}", log_value(&args.query, 1_000)))
                .unwrap_or_else(|error| invalid_arguments(&call.arguments, &error)),
            "read_file" => parse::<ReadFileArgs>(&call.arguments)
                .map(|args| {
                    let offset = args.offset.unwrap_or(1);
                    let limit = args.limit.unwrap_or(400).min(2_000);
                    let end = offset.saturating_add(limit).saturating_sub(1);
                    format!("path={} lines={offset}..={end}", self.log_path(&args.path))
                })
                .unwrap_or_else(|error| invalid_arguments(&call.arguments, &error)),
            "list_files" => parse::<PathArgs>(&call.arguments)
                .map(|args| {
                    format!(
                        "path={}",
                        self.log_path(args.path.as_deref().unwrap_or("."))
                    )
                })
                .unwrap_or_else(|error| invalid_arguments(&call.arguments, &error)),
            "search" => parse::<SearchArgs>(&call.arguments)
                .map(|args| {
                    format!(
                        "path={} query={}",
                        self.log_path(args.path.as_deref().unwrap_or(".")),
                        log_value(&args.query, 1_000)
                    )
                })
                .unwrap_or_else(|error| invalid_arguments(&call.arguments, &error)),
            "write_file" => parse::<WriteFileArgs>(&call.arguments)
                .map(|args| {
                    format!(
                        "path={} content_bytes={} content_preview={}",
                        self.log_path(&args.path),
                        args.content.len(),
                        log_value(&args.content, 160)
                    )
                })
                .unwrap_or_else(|error| invalid_arguments(&call.arguments, &error)),
            "edit_file" => parse::<EditFileArgs>(&call.arguments)
                .map(|args| {
                    format!(
                        "path={} replace_all={} old_text={} new_text={}",
                        self.log_path(&args.path),
                        args.replace_all,
                        log_value(&args.old_text, 160),
                        log_value(&args.new_text, 160)
                    )
                })
                .unwrap_or_else(|error| invalid_arguments(&call.arguments, &error)),
            "bash" => parse::<BashArgs>(&call.arguments)
                .map(|args| {
                    format!(
                        "cwd={} command={}",
                        log_value(&self.root.display().to_string(), 1_000),
                        log_value(&args.command, 2_000)
                    )
                })
                .unwrap_or_else(|error| invalid_arguments(&call.arguments, &error)),
            _ => format!("arguments={}", log_value(&call.arguments, 1_000)),
        }
    }

    async fn execute_inner(&self, call: &ToolCall) -> Result<String> {
        match call.name.as_str() {
            "read_file" => self.read_file(parse(&call.arguments)?),
            "list_files" => self.list_files(parse(&call.arguments)?).await,
            "search" => self.search(parse(&call.arguments)?).await,
            "write_file" if self.allow_write => self.write_file(parse(&call.arguments)?),
            "edit_file" if self.allow_write => self.edit_file(parse(&call.arguments)?),
            "bash" if self.allow_shell => self.bash(parse(&call.arguments)?).await,
            "write_file" | "edit_file" => Err(anyhow::anyhow!("file writes are disabled")),
            "bash" => Err(anyhow::anyhow!("shell execution is disabled")),
            other => Err(anyhow::anyhow!("unknown tool: {other}")),
        }
    }

    fn read_file(&self, args: ReadFileArgs) -> Result<String> {
        let path = self.existing_path(&args.path)?;
        if !path.is_file() {
            bail!("not a file: {}", args.path);
        }
        let content =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let offset = args.offset.unwrap_or(1);
        let limit = args.limit.unwrap_or(400).min(2_000);
        let lines = content
            .lines()
            .enumerate()
            .skip(offset.saturating_sub(1))
            .take(limit)
            .map(|(index, line)| format!("{}: {line}", index + 1))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(if lines.is_empty() {
            "(no lines in requested range)".into()
        } else {
            lines
        })
    }

    async fn list_files(&self, args: PathArgs) -> Result<String> {
        let path = self.existing_path(args.path.as_deref().unwrap_or("."))?;
        let path = self.command_path(&path)?;
        self.run_command("rg", [OsStr::new("--files"), path.as_os_str()])
            .await
    }

    async fn search(&self, args: SearchArgs) -> Result<String> {
        if args.query.is_empty() {
            bail!("query must not be empty");
        }
        let path = self.existing_path(args.path.as_deref().unwrap_or("."))?;
        let path = self.command_path(&path)?;
        self.run_command(
            "rg",
            [
                OsStr::new("--line-number"),
                OsStr::new("--column"),
                OsStr::new("--no-heading"),
                OsStr::new("--color=never"),
                OsStr::new("--max-count=200"),
                OsStr::new("--"),
                OsStr::new(&args.query),
                path.as_os_str(),
            ],
        )
        .await
    }

    fn write_file(&self, args: WriteFileArgs) -> Result<String> {
        let path = self.write_path(&args.path)?;
        let parent = path.parent().context("target path has no parent")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("create parent directory {}", parent.display()))?;
        fs::write(&path, args.content.as_bytes())
            .with_context(|| format!("write {}", path.display()))?;
        Ok(format!(
            "wrote {} bytes to {}",
            args.content.len(),
            args.path
        ))
    }

    fn edit_file(&self, args: EditFileArgs) -> Result<String> {
        if args.old_text.is_empty() {
            bail!("old_text must not be empty");
        }
        let path = self.existing_path(&args.path)?;
        if !path.is_file() {
            bail!("not a file: {}", args.path);
        }
        let content =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let occurrences = content.matches(&args.old_text).count();
        if occurrences == 0 {
            bail!("old_text was not found in {}", args.path);
        }
        if occurrences > 1 && !args.replace_all {
            bail!(
                "old_text occurs {occurrences} times in {}; provide a unique match or set replace_all",
                args.path
            );
        }
        let updated = if args.replace_all {
            content.replace(&args.old_text, &args.new_text)
        } else {
            content.replacen(&args.old_text, &args.new_text, 1)
        };
        fs::write(&path, updated.as_bytes())
            .with_context(|| format!("write {}", path.display()))?;
        Ok(format!(
            "replaced {occurrences} occurrence(s) in {}",
            args.path
        ))
    }

    async fn bash(&self, args: BashArgs) -> Result<String> {
        if args.command.trim().is_empty() {
            bail!("command must not be empty");
        }
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("cmd");
            command.args(["/C", &args.command]);
            command
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut command = Command::new("sh");
            command.args(["-lc", &args.command]);
            command
        };
        command
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let output = self.run_process(command, "shell command").await?;
        Ok(format_output(output.code, &output.stdout, &output.stderr))
    }

    async fn run_command<I, S>(&self, program: &str, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let output = self.run_process(command, program).await?;
        if output.success || output.code == Some(1) {
            let formatted = format_output(output.code, &output.stdout, &output.stderr);
            if output.code == Some(1) && output.stdout.is_empty() {
                Ok("(no matches)".into())
            } else {
                Ok(formatted)
            }
        } else {
            bail!(
                "{}",
                format_output(output.code, &output.stdout, &output.stderr)
            );
        }
    }

    async fn run_process(&self, mut command: Command, label: &str) -> Result<CapturedOutput> {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let (mut child, process_group) = self
            .process_scope
            .spawn(command)
            .with_context(|| format!("start {label}"))?;
        let stdout = child.stdout.take().context("capture child stdout")?;
        let stderr = child.stderr.take().context("capture child stderr")?;
        let limit = self.max_output_bytes.saturating_add(1);
        let capture = async {
            let (status, stdout, stderr) = tokio::try_join!(
                child.wait(),
                drain_limited(stdout, limit),
                drain_limited(stderr, limit),
            )?;
            Ok::<_, std::io::Error>(CapturedOutput {
                success: status.success(),
                code: status.code(),
                stdout,
                stderr,
            })
        };
        match timeout(self.timeout, capture).await {
            Ok(result) => result.with_context(|| format!("run {label}")),
            Err(_) => {
                let _ = process_group.kill();
                let _ = child.wait().await;
                bail!("{label} timed out")
            }
        }
    }

    fn existing_path(&self, input: &str) -> Result<PathBuf> {
        let relative = safe_relative(input)?;
        let path = self.root.join(relative);
        let canonical =
            dunce::canonicalize(&path).with_context(|| format!("path does not exist: {input}"))?;
        self.ensure_inside(canonical)
    }

    fn write_path(&self, input: &str) -> Result<PathBuf> {
        let relative = safe_relative(input)?;
        let path = self.root.join(relative);
        if path.exists() {
            return self.ensure_inside(
                dunce::canonicalize(&path).with_context(|| format!("resolve path: {input}"))?,
            );
        }
        let mut ancestor = path.parent();
        while let Some(parent) = ancestor {
            if parent.exists() {
                let canonical = dunce::canonicalize(parent)
                    .with_context(|| format!("resolve parent path: {}", parent.display()))?;
                self.ensure_inside(canonical)?;
                return Ok(path);
            }
            ancestor = parent.parent();
        }
        bail!("could not resolve a parent directory for {input}")
    }

    fn ensure_inside(&self, path: PathBuf) -> Result<PathBuf> {
        if path.starts_with(&self.root) {
            Ok(path)
        } else {
            bail!("path escapes workspace root: {}", path.display())
        }
    }

    fn command_path(&self, path: &Path) -> Result<PathBuf> {
        let relative = path
            .strip_prefix(&self.root)
            .context("tool path is outside the workspace root")?;
        Ok(if relative.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            relative.to_owned()
        })
    }

    fn log_path(&self, input: &str) -> String {
        let display = safe_relative(input)
            .map(|relative| self.root.join(relative).display().to_string())
            .unwrap_or_else(|_| input.to_owned());
        log_value(&display, 1_000)
    }
}

fn spec(name: &str, description: &str, parameters: serde_json::Value) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: Some(description.into()),
        parameters,
    }
}

fn parse<T: for<'de> Deserialize<'de>>(arguments: &str) -> Result<T> {
    serde_json::from_str(arguments).context("parse tool arguments")
}

fn invalid_arguments(arguments: &str, error: &anyhow::Error) -> String {
    format!(
        "arguments={} parse_error={}",
        log_value(arguments, 1_000),
        log_value(&error.to_string(), 300)
    )
}

fn log_value(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut preview = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        preview.push('…');
    }
    serde_json::to_string(&preview).unwrap_or_else(|_| "\"<unprintable>\"".into())
}

fn safe_relative(input: &str) -> Result<PathBuf> {
    if input.trim().is_empty() {
        bail!("path must not be empty");
    }
    let path = Path::new(input);
    if path.is_absolute() {
        bail!("absolute paths are not allowed: {input}");
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("path traversal is not allowed: {input}")
            }
        }
    }
    Ok(clean)
}

fn format_output(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> String {
    format!(
        "exit_code: {}\nstdout:\n{}\nstderr:\n{}",
        code.map_or_else(|| "signal".into(), |code| code.to_string()),
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    )
}

async fn drain_limited(
    mut reader: impl AsyncRead + Unpin,
    limit: usize,
) -> std::io::Result<Vec<u8>> {
    let mut retained = Vec::with_capacity(limit.min(8 * 1024));
    let mut buffer = [0u8; 8 * 1024];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(retained.len());
        retained.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    Ok(retained)
}

struct CapturedOutput {
    success: bool,
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn truncate(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    const SUFFIX: &str = "\n[truncated]";
    if max_bytes <= SUFFIX.len() {
        return SUFFIX[..max_bytes].to_owned();
    }
    let mut end = max_bytes - SUFFIX.len();
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{SUFFIX}", &value[..end])
}

#[derive(Deserialize)]
struct ReadFileArgs {
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct PathArgs {
    path: Option<String>,
}

#[derive(Deserialize)]
struct SearchArgs {
    query: String,
    path: Option<String>,
}

#[derive(Deserialize)]
struct WebSearchArgs {
    query: String,
}

#[derive(Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct EditFileArgs {
    path: String,
    old_text: String,
    new_text: String,
    #[serde(default)]
    replace_all: bool,
}

#[derive(Deserialize)]
struct BashArgs {
    command: String,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: Arc::from("call-1"),
            name: name.into(),
            arguments: Arc::from(arguments.to_string()),
        }
    }

    #[test]
    fn specs_only_advertise_enabled_mutations() {
        let root = tempfile::tempdir().unwrap();
        let read_only = ToolRuntime::new(root.path().to_owned(), false, false, 5, 4096);
        let names = read_only
            .specs()
            .into_iter()
            .map(|spec| spec.name)
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [WEB_SEARCH_TOOL_NAME, "read_file", "list_files", "search"]
        );

        let enabled = ToolRuntime::new(root.path().to_owned(), true, true, 5, 4096);
        let names = enabled
            .specs()
            .into_iter()
            .map(|spec| spec.name)
            .collect::<Vec<_>>();
        assert!(names.contains(&"write_file".to_owned()));
        assert!(names.contains(&"edit_file".to_owned()));
        assert!(names.contains(&"bash".to_owned()));
    }

    #[test]
    fn tool_descriptions_include_targets_and_inputs() {
        let root = tempfile::tempdir().unwrap();
        let canonical_root = dunce::canonicalize(root.path()).unwrap();
        let tools = ToolRuntime::new(canonical_root.clone(), true, true, 5, 4096);
        let root_text = canonical_root.display().to_string();

        let read = tools.describe_call(&call(
            "read_file",
            json!({"path":"src/main.rs","offset":12,"limit":20}),
        ));
        assert!(read.contains(&format!("{root_text}/src/main.rs")), "{read}");
        assert!(read.contains("lines=12..=31"), "{read}");

        let search = tools.describe_call(&call("search", json!({"path":"src","query":"fn main"})));
        assert!(search.contains(&format!("{root_text}/src")), "{search}");
        assert!(search.contains("query=\"fn main\""), "{search}");

        let write = tools.describe_call(&call(
            "write_file",
            json!({"path":"notes.txt","content":"first line\nsecond line"}),
        ));
        assert!(write.contains("content_bytes=22"), "{write}");
        assert!(
            write.contains("content_preview=\"first line\\nsecond line\""),
            "{write}"
        );

        let shell = tools.describe_call(&call("bash", json!({"command":"printf 'hello world'"})));
        assert!(shell.contains(&format!("cwd=\"{root_text}\"")), "{shell}");
        assert!(
            shell.contains("command=\"printf 'hello world'\""),
            "{shell}"
        );
    }

    #[tokio::test]
    async fn write_and_edit_stay_inside_workspace() {
        let root = tempfile::tempdir().unwrap();
        let tools = ToolRuntime::new(
            dunce::canonicalize(root.path()).unwrap(),
            true,
            false,
            5,
            4096,
        );
        let wrote = tools
            .execute(&call(
                "write_file",
                json!({"path": "src/demo.txt", "content": "one\ntwo\n"}),
            ))
            .await;
        assert!(wrote.starts_with("wrote "), "{wrote}");

        let edited = tools
            .execute(&call(
                "edit_file",
                json!({"path": "src/demo.txt", "old_text": "two", "new_text": "three"}),
            ))
            .await;
        assert!(edited.starts_with("replaced 1"), "{edited}");
        assert_eq!(
            fs::read_to_string(root.path().join("src/demo.txt")).unwrap(),
            "one\nthree\n"
        );

        let escaped = tools
            .execute(&call(
                "write_file",
                json!({"path": "../escaped.txt", "content": "no"}),
            ))
            .await;
        assert!(
            escaped.contains("path traversal is not allowed"),
            "{escaped}"
        );
    }

    #[tokio::test]
    async fn disabled_shell_is_rejected_even_if_called() {
        let root = tempfile::tempdir().unwrap();
        let tools = ToolRuntime::new(
            dunce::canonicalize(root.path()).unwrap(),
            false,
            false,
            5,
            4096,
        );
        let output = tools
            .execute(&call("bash", json!({"command": "printf unsafe"})))
            .await;
        assert_eq!(output, "Error: shell execution is disabled");
    }

    #[tokio::test]
    async fn listed_files_are_workspace_relative() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/demo.rs"), "fn main() {}\n").unwrap();
        let canonical_root = dunce::canonicalize(root.path()).unwrap();
        let tools = ToolRuntime::new(canonical_root.clone(), false, false, 5, 4096);
        let output = tools
            .execute(&call("list_files", json!({"path": "."})))
            .await;
        let normalized = output.replace('\\', "/");

        assert!(normalized.contains("src/demo.rs"), "{output}");
        assert!(
            !output.contains(&canonical_root.display().to_string()),
            "{output}"
        );
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn shell_output_is_bounded() {
        let root = tempfile::tempdir().unwrap();
        let tools = ToolRuntime::new(
            dunce::canonicalize(root.path()).unwrap(),
            false,
            true,
            5,
            128,
        );
        let output = tools
            .execute(&call("bash", json!({"command": "yes x | head -c 100000"})))
            .await;
        assert!(output.len() <= 128, "{} bytes", output.len());
        assert!(output.ends_with("[truncated]"), "{output}");
    }
}
