use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::task::{new_id, Task};

/// TOML ファイル全体のラッパー。[[task]] 配列を保持する。
#[derive(Debug, Default, Serialize, Deserialize)]
struct TasksFile {
    #[serde(default)]
    task: Vec<Task>,
}

/// tasks.toml から全タスクを読み込む。ファイル不在は空 Vec。
pub fn load(path: &Path) -> Result<Vec<Task>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let file: TasksFile =
        toml::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(file.task)
}

/// Vec<Task> を tasks.toml に書き戻す (アトミック書き込み: 一時ファイル→rename)。
pub fn save(path: &Path, tasks: &[Task]) -> Result<()> {
    let file = TasksFile {
        task: tasks.to_vec(),
    };
    let text =
        toml::to_string_pretty(&file).context("failed to serialize tasks to TOML")?;

    // 一時ファイルは同ディレクトリに置いて rename でアトミック差し替え
    let tmp_path = path.with_file_name(".tasks.toml.tmp");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    std::fs::write(&tmp_path, &text)
        .with_context(|| format!("failed to write tmp file {}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, path)
        .with_context(|| format!("failed to rename {} -> {}", tmp_path.display(), path.display()))?;
    Ok(())
}

/// タスクを追加して保存。生成した id を返す。
pub fn add(
    path: &Path,
    title: &str,
    project: &str,
    tags: Vec<String>,
    notes: &str,
    now: i64,
) -> Result<String> {
    let mut tasks = load(path)?;
    let id = new_id(title, now);
    let task = Task {
        id: id.clone(),
        title: title.to_string(),
        project: project.to_string(),
        tags,
        status: "pending".to_string(),
        notes: notes.to_string(),
        created_at: now,
        updated_at: now,
        defer_until: None,
    };
    tasks.push(task);
    save(path, &tasks)?;
    Ok(id)
}

/// pending/failed タスクを優先度順 (priority() 昇順、同優先度は created_at 昇順) で返す。
/// tag_filter: Some(tag) なら tags にそのタグを含むものだけ。
/// project_filter: Some(project) ならプロジェクトが一致するものだけ (repo_root との比較)。
/// defer_until が未来のタスク (is_deferred) はスキップする。
pub fn next(
    path: &Path,
    tag_filter: Option<&str>,
    project_filter: Option<&str>,
) -> Result<Option<Task>> {
    let now = now_unix();
    let tasks = load(path)?;
    let mut candidates: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.is_pending())
        .filter(|t| !t.is_deferred(now))
        .filter(|t| match tag_filter {
            Some(tag) => t.tags.iter().any(|tg| tg == tag),
            None => true,
        })
        .filter(|t| match project_filter {
            Some(proj) => project_matches(&t.project, proj),
            None => true,
        })
        .collect();

    candidates.sort_by_key(|t| (t.priority(), t.created_at));
    Ok(candidates.first().map(|t| (*t).clone()))
}

/// defer_until <= now のタスクの defer_until を None にクリアして status を "pending" に戻す。
/// 変更したタスクの件数を返す。
pub fn requeue_expired(path: &Path, now: i64) -> Result<usize> {
    let mut tasks = load(path)?;
    let mut count = 0usize;
    for task in tasks.iter_mut() {
        if let Some(defer_until) = task.defer_until {
            if defer_until <= now {
                task.defer_until = None;
                task.status = "pending".to_string();
                task.updated_at = now;
                count += 1;
            }
        }
    }
    if count > 0 {
        save(path, &tasks)?;
    }
    Ok(count)
}

/// id で特定のタスクを done に更新して保存。見つからなければエラー。
pub fn mark_done(path: &Path, id: &str) -> Result<()> {
    let mut tasks = load(path)?;
    let task = tasks
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| anyhow!("task not found: {}", id))?;
    task.status = "done".to_string();
    // updated_at はシステム時刻で更新（呼び出し元が now を持たないため現在時刻を使う）
    task.updated_at = now_unix();
    save(path, &tasks)
}

/// id で特定のタスクを failed に更新。reason を notes に追記。
/// defer_until を now + 172800 (2日) に設定してタスクを一時保留にする。
pub fn mark_failed(path: &Path, id: &str, reason: Option<&str>) -> Result<()> {
    let mut tasks = load(path)?;
    let task = tasks
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| anyhow!("task not found: {}", id))?;
    task.status = "failed".to_string();
    if let Some(r) = reason {
        if task.notes.is_empty() {
            task.notes = r.to_string();
        } else {
            task.notes.push('\n');
            task.notes.push_str(r);
        }
    }
    let now = now_unix();
    task.defer_until = Some(now + 172_800);
    task.updated_at = now;
    save(path, &tasks)
}

/// フィールドの一部を更新して保存。None のフィールドは変更しない。
pub fn edit(
    path: &Path,
    id: &str,
    title: Option<&str>,
    tags: Option<Vec<String>>,
    notes: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    let mut tasks = load(path)?;
    let task = tasks
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| anyhow!("task not found: {}", id))?;
    if let Some(v) = title {
        task.title = v.to_string();
    }
    if let Some(v) = tags {
        task.tags = v;
    }
    if let Some(v) = notes {
        task.notes = v.to_string();
    }
    if let Some(v) = status {
        task.status = v.to_string();
    }
    task.updated_at = now_unix();
    save(path, &tasks)
}

/// タスク一覧を返す。フィルタは all None で全件。
pub fn list(
    path: &Path,
    tag_filter: Option<&str>,
    project_filter: Option<&str>,
    status_filter: Option<&str>,
) -> Result<Vec<Task>> {
    let tasks = load(path)?;
    let result = tasks
        .into_iter()
        .filter(|t| match tag_filter {
            Some(tag) => t.tags.iter().any(|tg| tg == tag),
            None => true,
        })
        .filter(|t| match project_filter {
            Some(proj) => project_matches(&t.project, proj),
            None => true,
        })
        .filter(|t| match status_filter {
            Some(s) => t.status == s,
            None => true,
        })
        .collect();
    Ok(result)
}

// ---- helpers ----------------------------------------------------------------

/// project_filter のマッチング:
/// Task.project が filter と完全一致、または filter で始まる場合にマッチ。
fn project_matches(task_project: &str, filter: &str) -> bool {
    if task_project == filter {
        return true;
    }
    // filter が末尾スラッシュなしの repo_root の場合を考慮
    // task_project が filter + "/" で始まればマッチ
    if task_project.starts_with(filter) {
        let rest = &task_project[filter.len()..];
        return rest.starts_with('/');
    }
    false
}

/// 現在の Unix タイムスタンプ (秒)。
fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_path() -> PathBuf {
        let dir = tempfile::tempdir().expect("tmp dir");
        // keep the dir alive by leaking — acceptable in tests
        let path = dir.path().join("tasks.toml");
        std::mem::forget(dir);
        path
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let path = PathBuf::from("/nonexistent/tasks.toml");
        let tasks = load(&path).unwrap();
        assert!(tasks.is_empty());
    }

    #[test]
    fn add_and_load_roundtrip() {
        let path = tmp_path();
        let id = add(&path, "Test task", "/repo", vec!["p1".into()], "notes", 1000).unwrap();
        assert_eq!(id.len(), 8);
        let tasks = load(&path).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, id);
        assert_eq!(tasks[0].title, "Test task");
        assert_eq!(tasks[0].status, "pending");
    }

    #[test]
    fn next_returns_highest_priority() {
        let path = tmp_path();
        add(&path, "Low", "/repo", vec!["p2".into()], "", 100).unwrap();
        add(&path, "High", "/repo", vec!["p0".into()], "", 200).unwrap();
        add(&path, "Mid", "/repo", vec!["p1".into()], "", 150).unwrap();
        let t = next(&path, None, None).unwrap().unwrap();
        assert_eq!(t.title, "High");
    }

    #[test]
    fn next_same_priority_by_created_at() {
        let path = tmp_path();
        add(&path, "B", "/repo", vec!["p1".into()], "", 200).unwrap();
        add(&path, "A", "/repo", vec!["p1".into()], "", 100).unwrap();
        let t = next(&path, None, None).unwrap().unwrap();
        assert_eq!(t.title, "A");
    }

    #[test]
    fn next_skips_done_tasks() {
        let path = tmp_path();
        let id = add(&path, "Done task", "/repo", vec![], "", 100).unwrap();
        mark_done(&path, &id).unwrap();
        let result = next(&path, None, None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn mark_done_updates_status() {
        let path = tmp_path();
        let id = add(&path, "Task", "/repo", vec![], "", 100).unwrap();
        mark_done(&path, &id).unwrap();
        let tasks = load(&path).unwrap();
        assert_eq!(tasks[0].status, "done");
    }

    #[test]
    fn mark_done_unknown_id_errors() {
        let path = tmp_path();
        add(&path, "Task", "/repo", vec![], "", 100).unwrap();
        assert!(mark_done(&path, "nonexistent").is_err());
    }

    #[test]
    fn mark_failed_appends_reason() {
        let path = tmp_path();
        let id = add(&path, "Task", "/repo", vec![], "existing note", 100).unwrap();
        mark_failed(&path, &id, Some("timeout")).unwrap();
        let tasks = load(&path).unwrap();
        assert_eq!(tasks[0].status, "failed");
        assert!(tasks[0].notes.contains("timeout"));
        assert!(tasks[0].notes.contains("existing note"));
    }

    #[test]
    fn edit_updates_fields() {
        let path = tmp_path();
        let id = add(&path, "Old title", "/repo", vec![], "", 100).unwrap();
        edit(&path, &id, Some("New title"), None, Some("new notes"), None).unwrap();
        let tasks = load(&path).unwrap();
        assert_eq!(tasks[0].title, "New title");
        assert_eq!(tasks[0].notes, "new notes");
        assert_eq!(tasks[0].tags.len(), 0); // unchanged
    }

    #[test]
    fn list_with_status_filter() {
        let path = tmp_path();
        let id = add(&path, "Task A", "/repo", vec![], "", 100).unwrap();
        add(&path, "Task B", "/repo", vec![], "", 200).unwrap();
        mark_done(&path, &id).unwrap();
        let pending = list(&path, None, None, Some("pending")).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].title, "Task B");
    }

    #[test]
    fn list_with_tag_filter() {
        let path = tmp_path();
        add(&path, "Tagged", "/repo", vec!["bug".into()], "", 100).unwrap();
        add(&path, "Untagged", "/repo", vec![], "", 200).unwrap();
        let result = list(&path, Some("bug"), None, None).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Tagged");
    }

    #[test]
    fn project_filter_exact_match() {
        assert!(project_matches("/repo/foo", "/repo/foo"));
    }

    #[test]
    fn project_filter_prefix_with_slash() {
        assert!(project_matches("/repo/foo/bar", "/repo/foo"));
    }

    #[test]
    fn project_filter_no_match() {
        assert!(!project_matches("/repo/foobar", "/repo/foo"));
    }

    #[test]
    fn next_with_project_filter() {
        let path = tmp_path();
        add(&path, "In repo", "/repo/proj", vec![], "", 100).unwrap();
        add(&path, "Other", "/other/proj", vec![], "", 100).unwrap();
        let t = next(&path, None, Some("/repo/proj")).unwrap().unwrap();
        assert_eq!(t.title, "In repo");
    }

    #[test]
    fn mark_failed_sets_defer_until() {
        let path = tmp_path();
        let id = add(&path, "Task", "/repo", vec![], "", 100).unwrap();
        mark_failed(&path, &id, Some("error")).unwrap();
        let tasks = load(&path).unwrap();
        assert_eq!(tasks[0].status, "failed");
        // defer_until は Some で、now より未来であること
        let defer = tasks[0].defer_until.expect("defer_until should be set");
        // 172800 秒 (2日) 後を設定しているため now + 172800 付近であること
        assert!(defer > now_unix());
    }

    #[test]
    fn next_skips_deferred_task() {
        let path = tmp_path();
        let id = add(&path, "Will fail", "/repo", vec![], "", 1000).unwrap();
        // mark_failed でタスクが defer される
        mark_failed(&path, &id, None).unwrap();
        // deferred なので next は None を返す
        let result = next(&path, None, None).unwrap();
        assert!(result.is_none(), "deferred task should not be returned by next");
    }

    #[test]
    fn requeue_expired_restores_pending() {
        let path = tmp_path();
        let id = add(&path, "Task", "/repo", vec![], "", 1000).unwrap();
        mark_failed(&path, &id, None).unwrap();

        // defer 直後は next がスキップ
        assert!(next(&path, None, None).unwrap().is_none());

        // 期限を過去に設定するため、直接 load → edit → save する
        let mut tasks = load(&path).unwrap();
        tasks[0].defer_until = Some(500); // 過去のタイムスタンプ
        save(&path, &tasks).unwrap();

        // requeue_expired(now=1000) で期限切れタスクが復帰
        let count = requeue_expired(&path, 1000).unwrap();
        assert_eq!(count, 1);

        let tasks = load(&path).unwrap();
        assert_eq!(tasks[0].status, "pending");
        assert!(tasks[0].defer_until.is_none());

        // next でも取得できるようになる
        let t = next(&path, None, None).unwrap();
        assert!(t.is_some());
    }

    #[test]
    fn requeue_expired_returns_zero_when_none_expired() {
        let path = tmp_path();
        let id = add(&path, "Task", "/repo", vec![], "", 1000).unwrap();
        mark_failed(&path, &id, None).unwrap();
        // now を小さい値にして期限切れタスクがない状態でテスト
        let count = requeue_expired(&path, 0).unwrap();
        assert_eq!(count, 0);
    }
}
