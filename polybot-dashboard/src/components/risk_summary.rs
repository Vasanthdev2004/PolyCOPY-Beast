use crate::data;
use leptos::prelude::*;

#[component]
pub fn RiskSummary() -> impl IntoView {
    let paused = signal(false);
    let simulation = signal(true);
    let daily_pnl = signal(String::from("$0.00"));
    let open_positions = signal(0u32);
    let drawdown = signal(String::from("0.00%"));

    leptos::task::spawn_local({
        let paused = paused.1;
        let simulation = simulation.1;
        let daily_pnl = daily_pnl.1;
        let open_positions = open_positions.1;
        let drawdown = drawdown.1;
        async move {
            if let Ok(health) = data::fetch_health().await {
                paused.set(health.paused);
                simulation.set(health.simulation);
            }
            if let Ok(metrics) = data::fetch_metrics().await {
                daily_pnl.set(format!("${:.2}", metrics.daily_pnl_usd));
                open_positions.set(metrics.open_positions);
                drawdown.set(format!("{:.2}%", metrics.current_drawdown_pct * 100.0));
            }
        }
    });

    view! {
        <div>
            <div>
                <span>"Daily PnL"</span>
                <span>{move || daily_pnl.0.get()}</span>
            </div>
            <div>
                <span>"Status"</span>
                <span>{move || if paused.0.get() { "PAUSED" } else { "ACTIVE" }}</span>
            </div>
            <div>
                <span>"Mode"</span>
                <span>{ move || if simulation.0.get() { "SIMULATION" } else { "LIVE" }}</span>
            </div>
            <div>
                <span>"Open Positions"</span>
                <span>{move || open_positions.0.get()}</span>
            </div>
            <div>
                <span>"Drawdown"</span>
                <span>{move || drawdown.0.get()}</span>
            </div>
        </div>
    }
}
