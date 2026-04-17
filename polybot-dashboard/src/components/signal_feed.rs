use crate::data;
use leptos::prelude::*;

#[derive(Clone, Debug)]
struct SignalEntry {
    signal_id: String,
    wallet: String,
    secret_level: u8,
    confidence: u8,
    category: String,
    action: String,
    market: String,
    side: String,
}

#[component]
pub fn SignalFeed(#[prop(default = 20)] limit: usize) -> impl IntoView {
    let signals = RwSignal::new(Vec::<SignalEntry>::new());

    leptos::task::spawn_local({
        let signals = signals;
        async move {
            if let Ok(data) = data::fetch_signals(limit).await {
                signals.set(
                    data.into_iter()
                        .map(|signal| SignalEntry {
                            signal_id: signal.signal_id,
                            wallet: signal.wallet_address,
                            secret_level: signal.secret_level,
                            confidence: signal.confidence,
                            category: signal.category,
                            action: signal.disposition,
                            market: signal.market_id,
                            side: signal.side,
                        })
                        .collect(),
                );
            }
        }
    });

    view! {
        <div>
            <ul>
                <For
                    each=move || signals.get()
                    key=|signal| signal.signal_id.clone()
                    children=move |signal| {
                        view! {
                            <li>
                                {format!(
                                    "{} | {} | conf {} | secret {} | {} | {} | {}",
                                    signal.market,
                                    signal.side,
                                    signal.confidence,
                                    signal.secret_level,
                                    signal.category,
                                    signal.action,
                                    signal.wallet,
                                )}
                            </li>
                        }
                    }
                />
            </ul>
            <div>
                {move || {
                    if signals.get().is_empty() {
                        "No signals received yet. Waiting for scanner...".to_string()
                    } else {
                        String::new()
                    }
                }}
            </div>
        </div>
    }
}
