//! The Lattice services Alelyon runs, composed as a shipped build composes them, and the two ports the chat core
//! asks of a window.
//!
//! The chat core (`lattice-core`'s `AgentChat`, the agent chat behind `AgentChatService`) is built over the shared
//! chat store (`<globals>/lattice_chat`, the web Lattice's), the managed llama.cpp server (`LlamaRuntime`: Local and
//! Auto never leave the machine), and this window's ports. The runs service (`CoreService`) is the one the native
//! Lattice window shows: agent runs and their traces. Both run on a tokio runtime of their own with two worker
//! threads, made when the Lattice page is first opened; nothing is started before that.
//!
//! The core never draws: when a decision widens authority or sends data off the machine (trusting a folder, running a
//! command, the first send to a model off this computer, ...) it calls [`ConfirmPort::confirm`] itself, and only the
//! reader's click in the dialog this window draws answers yes. The dialog is drawn by this window's own Rust code (no
//! page script can reach it), from the core's own facts ([`ConfirmRequest::dialog`]): the refusing button is the
//! default, input is ignored for its first half second, and text from the model or a folder is shown escaped.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::channel::{mpsc, oneshot};
use futures::future::BoxFuture;
use futures::lock::Mutex as AsyncMutex;
use futures::stream::{BoxStream, StreamExt};

use lattice_core::convo::agent::{AgentChat, AgentConfig};
use lattice_core::llama::ManagedRuntime;
use lattice_core::llama::server::{LlamaRuntime, RuntimeConfig};
use lattice_core::ports::{AttentionPort, ConfirmPort, ConfirmRequest};
use lattice_core::{CoreConfig, CoreService, ProcessEnv, StateRoot};
use lattice_protocol::RunService;
use lattice_protocol::conversation::AgentChatService;

/// One decision the core asked of the reader, waiting for the dialog's answer.
pub struct Ask {
    pub request: ConfirmRequest,
    answer: Option<oneshot::Sender<bool>>,
}

impl Ask {
    /// Answer it once; a dropped ask answers no.
    pub fn answer(&mut self, yes: bool) {
        if let Some(tx) = self.answer.take() {
            let _ = tx.send(yes);
        }
    }
}

impl Drop for Ask {
    fn drop(&mut self) {
        self.answer(false);
    }
}

impl std::fmt::Debug for Ask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ask").field("request", &self.request).finish()
    }
}

/// The confirm port: each request becomes an [`Ask`] for the page, and the future answers when the reader does (no
/// when the window is gone).
struct WindowConfirm(mpsc::UnboundedSender<Ask>);

impl ConfirmPort for WindowConfirm {
    fn confirm(&self, request: ConfirmRequest) -> BoxFuture<'static, bool> {
        let (tx, rx) = oneshot::channel();
        if self.0.unbounded_send(Ask { request, answer: Some(tx) }).is_err() {
            return Box::pin(async { false });
        }
        Box::pin(async move { rx.await.unwrap_or(false) })
    }
}

/// The attention port: the conversations that asked for the reader, for the page to mark. It never carries text from
/// a conversation and never causes an action.
#[derive(Default)]
pub struct Attention(Mutex<Vec<String>>);

impl AttentionPort for Attention {
    fn attention(&self, conversation: &str) {
        let mut asked = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if !asked.iter().any(|c| c == conversation) {
            asked.push(conversation.to_string());
        }
    }
}

impl Attention {
    /// The conversations that asked since the last call.
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

/// The running services.
pub struct Services {
    pub chat: Arc<AgentChat>,
    pub runs: Arc<dyn RunService>,
    pub attention: Arc<Attention>,
    pub state: StateRoot,
    /// The dialogs the core asks for, read by the page's subscription (shared, so a subscription made again reads on
    /// from where the last one stopped).
    asks: Arc<AsyncMutex<mpsc::UnboundedReceiver<Ask>>>,
    runtime: Option<tokio::runtime::Runtime>,
}

impl std::fmt::Debug for Services {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Services").field("globals", &self.state.globals).finish()
    }
}

impl Services {
    /// Compose them, as a shipped build does. Blocking (it starts threads and reads the state root): call it off the
    /// window's thread.
    pub fn start() -> Result<Services, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("centcom-lattice")
            .enable_all()
            .build()
            .map_err(|e| format!("Lattice's worker threads could not start: {e}"))?;
        let state = lattice_core::state::resolve();
        let env: Arc<dyn lattice_core::Env> = Arc::new(ProcessEnv);
        let config = RuntimeConfig::new(env.clone(), &state)
            .map_err(|e| format!("the local model server's client could not be made: {e:?}"))?;
        let local: Arc<dyn ManagedRuntime> = Arc::new(LlamaRuntime::new(config, runtime.handle().clone()));
        let (tx, rx) = mpsc::unbounded();
        let attention = Arc::new(Attention::default());
        let chat_config =
            AgentConfig::new(state.clone(), env, local, Arc::new(WindowConfirm(tx)), attention.clone());
        let chat = Arc::new(AgentChat::new(chat_config, runtime.handle().clone()));
        let runs: Arc<dyn RunService> = {
            let _entered = runtime.enter();
            Arc::new(CoreService::new(CoreConfig::from_process(false)))
        };
        Ok(Services { chat, runs, attention, state, asks: Arc::new(AsyncMutex::new(rx)), runtime: Some(runtime) })
    }

    /// The services' own runtime: every chat call runs on it.
    pub fn handle(&self) -> tokio::runtime::Handle {
        self.runtime.as_ref().expect("the runtime lives as long as the services").handle().clone()
    }

    pub fn agent(&self) -> Arc<dyn AgentChatService> {
        self.chat.clone()
    }

    /// The dialogs the core asks for, one at a time, for as long as the services run.
    pub fn asks(&self) -> BoxStream<'static, Ask> {
        let shared = self.asks.clone();
        futures::stream::unfold(shared, |shared| async move {
            let next = shared.lock().await.next().await;
            next.map(|ask| (ask, shared))
        })
        .boxed()
    }
}

impl Drop for Services {
    fn drop(&mut self) {
        // Stop every answer and wait briefly for its save, then end the runtime (and with it the managed server's
        // job object, which ends the server).
        if let Some(runtime) = self.runtime.take() {
            let chat = self.chat.clone();
            runtime.block_on(async move { chat.shutdown(Duration::from_secs(5)).await });
            runtime.shutdown_timeout(Duration::from_secs(2));
        }
    }
}

/// An ask as the core makes one, for the page's tests.
#[cfg(test)]
pub fn tests_ask(request: ConfirmRequest, answer: oneshot::Sender<bool>) -> Ask {
    Ask { request, answer: Some(answer) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dropped_ask_answers_no_and_an_answered_one_answers_once() {
        let (tx, rx) = oneshot::channel();
        drop(Ask { request: ConfirmRequest::AttachFolder { path: "C:/x".into() }, answer: Some(tx) });
        assert_eq!(futures::executor::block_on(rx), Ok(false));

        let (tx, rx) = oneshot::channel();
        let mut ask = Ask { request: ConfirmRequest::AttachFolder { path: "C:/x".into() }, answer: Some(tx) };
        ask.answer(true);
        ask.answer(false);
        drop(ask);
        assert_eq!(futures::executor::block_on(rx), Ok(true));
    }

    #[test]
    fn the_confirm_port_answers_what_the_reader_answers_and_no_once_the_window_is_gone() {
        let (tx, mut rx) = mpsc::unbounded();
        let port = WindowConfirm(tx);
        let asked = port.confirm(ConfirmRequest::AttachFolder { path: "C:/x".into() });
        let mut ask = futures::executor::block_on(rx.next()).expect("the page receives the ask");
        ask.answer(true);
        assert!(futures::executor::block_on(asked));
        drop(rx);
        assert!(!futures::executor::block_on(port.confirm(ConfirmRequest::AttachFolder { path: "C:/y".into() })));
    }

    #[test]
    fn attention_names_each_conversation_once_and_forgets_on_take() {
        let attention = Attention::default();
        attention.attention("a");
        attention.attention("a");
        attention.attention("b");
        assert_eq!(attention.take(), vec!["a".to_string(), "b".to_string()]);
        assert!(attention.take().is_empty());
    }
}
