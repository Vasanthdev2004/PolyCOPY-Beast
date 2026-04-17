use crate::data;
use leptos::prelude::*;

#[derive(Clone, Debug)]
struct PositionEntry {
    id: String,
    market_id: String,
    side: String,
    average_price: String,
    current_size: String,
    category: String,
    status: String,
}

#[component]
pub fn PositionTable() -> impl IntoView {
    let positions = RwSignal::new(Vec::<PositionEntry>::new());

    leptos::task::spawn_local({
        let positions = positions;
        async move {
            if let Ok(data) = data::fetch_positions().await {
                positions.set(
                    data.into_iter()
                        .map(|pos| PositionEntry {
                            id: pos.id,
                            market_id: pos.market_id,
                            side: pos.side,
                            average_price: pos.average_price,
                            current_size: pos.current_size,
                            category: pos.category,
                            status: pos.status,
                        })
                        .collect(),
                );
            }
        }
    });

    view! {
        <div>
            <table>
                <thead>
                    <tr>
                        <th>"Market"</th>
                        <th>"Side"</th>
                        <th>"Avg Price"</th>
                        <th>"Size"</th>
                        <th>"Category"</th>
                        <th>"Status"</th>
                    </tr>
                </thead>
                <tbody>
                    <For
                        each=move || positions.get()
                        key=|pos| pos.id.clone()
                        children=move |pos| {
                            view! {
                                <tr>
                                    <td>{pos.market_id}</td>
                                    <td>{pos.side}</td>
                                    <td>{pos.average_price}</td>
                                    <td>{pos.current_size}</td>
                                    <td>{pos.category}</td>
                                    <td>{pos.status}</td>
                                </tr>
                            }
                        }
                    />
                </tbody>
            </table>
            <div>
                {move || {
                    if positions.get().is_empty() {
                        "No open positions".to_string()
                    } else {
                        String::new()
                    }
                }}
            </div>
        </div>
    }
}
