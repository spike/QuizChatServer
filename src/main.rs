use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use futures::{sink::SinkExt, stream::StreamExt};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use tokio::time::{timeout, Duration};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// Our shared state
struct AppState {
    user_set: Mutex<HashSet<String>>,
    tx: broadcast::Sender<String>,
    quiz_mode: Mutex<bool>,
    quiz_participants: Mutex<HashMap<String, Option<String>>>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "example_chat=trace".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let user_set = Mutex::new(HashSet::new());
    let (tx, _rx) = broadcast::channel(100);
    let quiz_mode = Mutex::new(false);
    let quiz_participants = Mutex::new(HashMap::new());

    let app_state = Arc::new(AppState {
        user_set,
        tx,
        quiz_mode,
        quiz_participants,
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/websocket", get(websocket_handler))
        .with_state(app_state);

    //let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
    let listener = tokio::net::TcpListener::bind("[::]:3000")
        .await
        .unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(socket, state))
}

async fn websocket(stream: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = stream.split();

    let mut username = String::new();
    while let Some(Ok(message)) = receiver.next().await {
        if let Message::Text(name) = message {
            check_username(&state, &mut username, &name);

            if !username.is_empty() {
                break;
            } else {
                let _ = sender
                    .send(Message::Text(String::from("Username already taken.")))
                    .await;

                return;
            }
        }
    }

    let sender = Arc::new(Mutex::new(sender));
    let mut rx = state.tx.subscribe();
    let msg = format!("{username} joined.");
    tracing::debug!("{msg}");
    let _ = state.tx.send(msg);

    let sender_clone = sender.clone();
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if let Err(_) = sender_clone.lock().await.send(Message::Text(msg)).await {
                break;
            }
        }
    });

    let tx = state.tx.clone();
    let name = username.clone();

    let state_clone = state.clone();
    let sender_clone = sender.clone();
    let mut recv_task = tokio::spawn(async move {
        loop {
            match timeout(Duration::from_secs(7200), receiver.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    if text == "/quiz" {
                        start_quiz(&state_clone, &name).await;
                    } else if text == "/show" {
                        show_answers(&state_clone).await;
                    } else {
                        let is_quiz_mode = *state_clone.quiz_mode.lock().unwrap();
                        if is_quiz_mode {
                            submit_answer(&state_clone, &name, &text).await;
                        } else {
                            let _ = tx.send(format!("{name}: {text}"));
                        }
                    }
                },
                Ok(Some(Ok(_))) => (),
                Ok(Some(Err(_))) | Ok(None) => break,
                Err(_) => {
                    let _ = sender_clone.lock().await.send(Message::Close(None)).await;
                    break;
                },
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    };

    let msg = format!("{username} left.");
    tracing::debug!("{msg}");
    let _ = state.tx.send(msg);

    state.user_set.lock().unwrap().remove(&username);
    state.quiz_participants.lock().unwrap().remove(&username);
}

fn check_username(state: &AppState, string: &mut String, name: &str) {
    let mut user_set = state.user_set.lock().unwrap();

    if !user_set.contains(name) {
        user_set.insert(name.to_owned());
        string.push_str(name);
    }
}

async fn start_quiz(state: &Arc<AppState>, name: &str) {
    let mut quiz_mode = state.quiz_mode.lock().unwrap();
    let mut quiz_participants = state.quiz_participants.lock().unwrap();

    *quiz_mode = true;
    quiz_participants.clear();
    quiz_participants.insert(name.to_string(), None);

    let msg = "Quiz mode started. Submit your answers.";
    let _ = state.tx.send(msg.to_string());
}

async fn submit_answer(state: &Arc<AppState>, name: &str, answer: &str) {
    let mut quiz_participants = state.quiz_participants.lock().unwrap();
    if let Some(entry) = quiz_participants.get_mut(name) {
        *entry = Some(answer.to_string());
    } else {
        quiz_participants.insert(name.to_string(), Some(answer.to_string()));
    }

    // let participants: Vec<String> = quiz_participants.keys().cloned().collect();
    // let _msg = format!("Current participants: {:?}", participants);
    // let _ = state.tx.send(msg);
    // let msg2 = format!("{} ", name.to_string());
    // let _ = state.tx.send(".".to_string());
}

async fn show_answers(state: &Arc<AppState>) {
    let mut quiz_mode = state.quiz_mode.lock().unwrap();
    let quiz_participants = state.quiz_participants.lock().unwrap();

    *quiz_mode = false;

    for (name, answer) in quiz_participants.iter() {
        let answer_text = answer.as_deref().unwrap_or("No answer submitted");
        let msg = format!("{name}: {answer_text}");
        let _ = state.tx.send(msg);
    }

    let msg = "Quiz mode ended.";
    let _ = state.tx.send(msg.to_string());
}

async fn index() -> Html<&'static str> {
    Html(std::include_str!("../chat.html"))
}
