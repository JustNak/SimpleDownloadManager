use crate::storage::DownloadPrompt;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

pub const PROMPT_CHANGED_EVENT: &str = "app://download-prompt-changed";

#[derive(Debug)]
pub enum PromptDecision {
    Download {
        directory_override: Option<String>,
        allow_duplicate: bool,
    },
    ShowExisting,
    Cancel,
}

#[derive(Clone, Default)]
pub struct PromptRegistry {
    inner: Arc<Mutex<PromptRegistryInner>>,
}

#[derive(Default)]
struct PromptRegistryInner {
    active_id: Option<String>,
    order: VecDeque<String>,
    prompts: HashMap<String, PendingPrompt>,
}

struct PendingPrompt {
    prompt: DownloadPrompt,
    responder: Option<oneshot::Sender<PromptDecision>>,
}

impl PromptRegistry {
    pub async fn enqueue(&self, prompt: DownloadPrompt) -> oneshot::Receiver<PromptDecision> {
        let (sender, receiver) = oneshot::channel();
        let mut inner = self.inner.lock().await;
        let id = prompt.id.clone();
        if inner.active_id.is_none() {
            inner.active_id = Some(id.clone());
        } else {
            inner.order.push_back(id.clone());
        }
        inner.prompts.insert(
            id,
            PendingPrompt {
                prompt,
                responder: Some(sender),
            },
        );
        receiver
    }

    pub async fn active_prompt(&self) -> Option<DownloadPrompt> {
        let inner = self.inner.lock().await;
        active_prompt(&inner)
    }

    pub async fn resolve(
        &self,
        id: &str,
        decision: PromptDecision,
    ) -> Result<Option<DownloadPrompt>, String> {
        let mut inner = self.inner.lock().await;
        if inner.active_id.as_deref() != Some(id) {
            return Err("The requested download prompt is not active.".into());
        }

        let Some(mut pending) = inner.prompts.remove(id) else {
            return Err("Download prompt was not found.".into());
        };
        inner.active_id = inner.order.pop_front();
        if let Some(sender) = pending.responder.take() {
            let _ = sender.send(decision);
        }

        Ok(active_prompt(&inner))
    }
}

fn active_prompt(inner: &PromptRegistryInner) -> Option<DownloadPrompt> {
    inner
        .active_id
        .as_ref()
        .and_then(|id| inner.prompts.get(id))
        .map(|pending| pending.prompt.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn queued_prompts_become_active_after_current_decision() {
        let registry = PromptRegistry::default();
        let first_receiver = registry.enqueue(prompt("prompt_1")).await;
        let _second_receiver = registry.enqueue(prompt("prompt_2")).await;

        assert_eq!(
            registry.active_prompt().await.map(|prompt| prompt.id),
            Some("prompt_1".into())
        );

        let next = registry
            .resolve("prompt_1", PromptDecision::Cancel)
            .await
            .expect("active prompt should resolve");

        assert!(matches!(first_receiver.await, Ok(PromptDecision::Cancel)));
        assert_eq!(next.map(|prompt| prompt.id), Some("prompt_2".into()));
        assert_eq!(
            registry.active_prompt().await.map(|prompt| prompt.id),
            Some("prompt_2".into())
        );
    }

    fn prompt(id: &str) -> DownloadPrompt {
        DownloadPrompt {
            id: id.into(),
            url: format!("https://example.com/{id}.zip"),
            filename: format!("{id}.zip"),
            source: None,
            total_bytes: None,
            default_directory: "C:/Downloads".into(),
            target_path: format!("C:/Downloads/{id}.zip"),
            duplicate_job: None,
        }
    }
}
