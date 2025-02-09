use axum::{
    extract::{Path, State}, routing::{get, post}, Json, Router
};
use serde::Deserialize;
use tokio::task::spawn_blocking;
use crate::qecu::emulator::Emulator;

use super::interceptor::{CodeHook, EventCallback};

#[derive(Clone)]
struct AppState {
    emulator: Emulator<'static>
}

#[derive(Deserialize)]
struct EmitEvent {
    msg: String
}

async fn emit(Path(event_type): Path<String>, State(state): State<AppState>, Json(payload): Json<EmitEvent>) -> String {
    spawn_blocking(move || {
        let emu = state.emulator;
        emu.emit(event_type, payload.msg);
    }).await.expect("[qecu::api::emit] spawn_blocking error.");
    return String::from("OK");
}

async fn interceptor_get_code_hooks(State(state): State<AppState>) -> Json<Vec<CodeHook>> {
    let code_hooks = spawn_blocking(move || {
        let emu: Emulator<'static> = state.emulator;
        let mut lock = emu.interceptor.lock();
        let intercept = lock.as_mut().unwrap().as_mut().unwrap();
        intercept.get_code_hooks().clone()
    }).await.unwrap();
    Json(code_hooks)
}

async fn interceptor_get_event_hooks(State(state): State<AppState>) -> Json<Vec<EventCallback>> {
    let event_hooks = spawn_blocking(move || {
        let emu: Emulator<'static> = state.emulator;
        let mut lock = emu.interceptor.lock();
        let intercept = lock.as_mut().unwrap().as_mut().unwrap();
        intercept.get_event_hooks().clone()
    }).await.unwrap();
    Json(event_hooks)
}

pub async fn bootstrap(bind_addr: String, emulator: Emulator<'static>) {
    let app = Router::new()
                                .route("/emit/{event_type}", post(emit))
                                .route("/interceptor/hooks/code", get(interceptor_get_code_hooks))
                                .route("/interceptor/hooks/events", get(interceptor_get_event_hooks))
                            .with_state(AppState { emulator: emulator});
    let listener = tokio::net::TcpListener::bind(bind_addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}