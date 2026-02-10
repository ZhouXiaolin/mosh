use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

/// Build the task-list section of the system prompt. The model must use **bash**
/// only for all task operations, via the four named bash tasks below.
pub fn format_task_prompt(task_file_path: &PathBuf) -> String {
    let path_str = task_file_path.display().to_string();
    let perl_cmd = format!(r#"perl -i -pe 's/^- \\[ \\] N\\./- [x] N./' "{}""#, path_str);
    format!(
        r#"

## Task List Protocol — Four Bash Tasks

**Task file path (use in every bash command below):**
`{path_str}`

All task operations are done by calling the **bash** tool with one of these four bash tasks. Do not output task content in your assistant text.

---

**1. TaskCreate** — 创建任务列表  
Call once in your first response to create the task list file. Use bash with a heredoc to write the checklist (numbered lines `- [ ] 1. ...`, one per step).

Example:
bash -c 'cat << ''EOF'' > "{path_str}"
# Tasks

- [ ] 1. First step description
- [ ] 2. Second step description
- [ ] 3. Third step
EOF'

---

**2. TaskUpdate** — 更新任务状态  
Call when a step is completed: mark item N as done (replace N with the step number).

bash -c "{perl_cmd}"

(Replace N in the perl script with the actual step number, e.g. 3 for step 3.)

---

**3. TaskList** — 列出所有任务  
Call to list all tasks (show the current task file content).

bash -c 'cat "{path_str}"'

---

**4. TaskGet** — 获取任务详情  
Call to get full content or a specific part of the task file (e.g. grep for one line).

bash -c 'cat "{path_str}"'
# or e.g. bash -c 'grep \"^\- \" \"{path_str}\"' for checklist lines only

---

Use TaskCreate in the first response; use TaskUpdate when a step is done; use TaskList/TaskGet when you need to read the current list. The UI monitors the file and refreshes automatically."#,
        path_str = path_str,
        perl_cmd = perl_cmd
    )
}

/// Parse task lines from assistant text, looking for the `<!-- TASKS ... TASKS -->` block.
pub fn parse_tasks(text: &str) -> Option<Vec<String>> {
    let start = text.find("<!-- TASKS")?;
    let end = text.find("TASKS -->")?;
    if end <= start {
        return None;
    }

    let block = &text[start + "<!-- TASKS".len()..end];
    let lines: Vec<String> = block
        .lines()
        .map(|l| l.trim())
        .filter(|l| l.starts_with("- ["))
        .map(|l| l.to_string())
        .collect();

    if lines.is_empty() { None } else { Some(lines) }
}

/// Get the project name from the current working directory.
fn project_name() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "unknown".to_string())
}

/// Return the path to the tasks directory, creating it if needed.
fn tasks_dir() -> Result<PathBuf> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
    let dir = home.join(".mash").join("tasks");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Initialize a new task file for this session, returning its path.
pub fn init_task_file() -> Result<PathBuf> {
    let dir = tasks_dir()?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = format!("{}_{}.md", project_name(), ts);
    let path = dir.join(name);
    // 仅写标题，不写占位符；模型在首条回复里用 TaskCreate 创建多步任务时才写入清单
    fs::write(&path, format!("# Tasks — {}\n\n", project_name()))?;
    Ok(path)
}

/// Write the current task list to the task file.
pub fn write_tasks(path: &PathBuf, lines: &[String]) -> Result<()> {
    let mut content = format!("# Tasks — {}\n\n", project_name());
    for line in lines {
        content.push_str(line);
        content.push('\n');
    }
    fs::write(path, content)?;
    Ok(())
}

/// Read full task file content for display in the UI (e.g. status line).
pub fn read_task_content(path: &PathBuf) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Read task summary from a task file: (completed, total).
pub fn read_task_summary(path: &PathBuf) -> Option<(usize, usize)> {
    let content = fs::read_to_string(path).ok()?;
    let total = content
        .lines()
        .filter(|l| l.trim().starts_with("- ["))
        .count();
    if total == 0 {
        return None;
    }
    let done = content
        .lines()
        .filter(|l| l.trim().starts_with("- [x]"))
        .count();
    Some((done, total))
}
