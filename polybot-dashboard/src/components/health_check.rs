use crate::data::{self, HealthData, MetricsData};
use leptos::prelude::*;

#[component]
pub fn HealthCheck() -> impl IntoView {
    let health = signal(Option::<HealthData>::None);
    let metrics = signal(Option::<MetricsData>::None);
    let error_msg = signal(String::new());
    let loading = signal(false);

    let refresh = move || {
        loading.1.set(true);
        error_msg.1.set(String::new());
        let h_writer = health.1;
        let m_writer = metrics.1;
        let e_writer = error_msg.1;
        let l_writer = loading.1;
        leptos::task::spawn_local(async move {
            match data::fetch_health().await {
                Ok(h) => h_writer.set(Some(h)),
                Err(e) => e_writer.set(e),
            }
            match data::fetch_metrics().await {
                Ok(m) => m_writer.set(Some(m)),
                Err(_e) => {
                    // Only set error if not already set by health fetch
                }
            }
            l_writer.set(false);
        });
    };

    // Initial fetch
    refresh();

    view! {
        <div>
            <h1>"System Health"</h1>
            <button on:click=move |_| refresh()>"Refresh"</button>
            {move || {
                let is_loading = loading.0.get();
                let err = error_msg.0.get();
                let h = health.0.get();
                let h2 = h.clone();
                let m = metrics.0.get();
                if is_loading {
                    view! { <p>"Loading..."</p> }.into_any()
                } else if !err.is_empty() {
                    let err_text = err.clone();
                    view! { <p style="color: red">{err_text}</p> }.into_any()
                } else {
                    view! {
                        <div>
                            <div>
                                <h2>"Overall Status"</h2>
                                {match h {
                                    Some(h) => {
                                        let status = h.status.clone();
                                        let uptime = h.uptime_secs;
                                        let sim = h.simulation;
                                        let paused = h.paused;
                                        let signals_received = h.signals_received;
                                        let signals_processed = h.signals_processed;
                                        let emergency_stops = h.emergency_stops;
                                        view! {
                                            <div>
                                                <div><span>"Status: "</span><span>{status}</span></div>
                                                <div><span>"Uptime: "</span><span>{format!("{}s", uptime)}</span></div>
                                                <div><span>"Mode: "</span><span>{if sim { "SIMULATION" } else { "LIVE" }}</span></div>
                                                <div><span>"Trading: "</span><span>{if paused { "PAUSED" } else { "Active" }}</span></div>
                                                <div><span>"Signals Received: "</span><span>{signals_received}</span></div>
                                                <div><span>"Signals Processed: "</span><span>{signals_processed}</span></div>
                                                <div><span>"Emergency Stops: "</span><span>{emergency_stops}</span></div>
                                            </div>
                                        }.into_any()
                                    },
                                    None => view! { <p>"No health data"</p> }.into_any(),
                                }}
                            </div>
                            <div>
                                <h2>"Connections"</h2>
                                {match h2 {
                                    Some(h) => {
                                        let redis = h.redis_connected;
                                        let ws = h.ws_connected;
                                        let rpc = h.rpc_status.clone();
                                        let last_signal = h
                                            .last_signal_at
                                            .clone()
                                            .unwrap_or_else(|| "Never".to_string());
                                        view! {
                                            <div>
                                                <div><span>"Redis: "</span><span>{if redis { "Connected" } else { "Disconnected" }}</span></div>
                                                <div><span>"CLOB WebSocket: "</span><span>{if ws { "Connected" } else { "Disconnected" }}</span></div>
                                                <div><span>"RPC: "</span><span>{rpc}</span></div>
                                                <div><span>"Last Signal: "</span><span>{last_signal}</span></div>
                                            </div>
                                        }.into_any()
                                    },
                                    None => view! { <p>"No connection data"</p> }.into_any(),
                                }}
                            </div>
                            <div>
                                <h2>"Metrics"</h2>
                                {match m {
                                    Some(m) => {
                                        let sig_recv = m.signals_received;
                                        let sig_proc = m.signals_processed;
                                        let trades = m.trades_executed;
                                        let pos = m.open_positions;
                                        let pnl = m.daily_pnl_usd;
                                        let drawdown = m.current_drawdown_pct;
                                        view! {
                                            <div>
                                                <div><span>"Signals Received: "</span><span>{sig_recv}</span></div>
                                                <div><span>"Signals Processed: "</span><span>{sig_proc}</span></div>
                                                <div><span>"Trades Executed: "</span><span>{trades}</span></div>
                                                <div><span>"Open Positions: "</span><span>{pos}</span></div>
                                                <div><span>"Daily PnL: "</span><span>{format!("${:.2}", pnl)}</span></div>
                                                <div><span>"Drawdown: "</span><span>{format!("{:.2}%", drawdown * 100.0)}</span></div>
                                            </div>
                                        }.into_any()
                                    },
                                    None => view! { <p>"No metrics data"</p> }.into_any(),
                                }}
                            </div>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}
