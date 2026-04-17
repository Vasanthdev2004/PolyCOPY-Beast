use crate::components::{PositionTable, RiskSummary, SignalFeed};
use leptos::prelude::*;

#[component]
pub fn Dashboard() -> impl IntoView {
    view! {
        <div>
            <h1>"SuperFast PolyBot v2 Dashboard"</h1>
            <div>
                <div>
                    <h2>"Risk Overview"</h2>
                    <RiskSummary/>
                </div>
                <div>
                    <h2>"Recent Signals"</h2>
                    <SignalFeed limit=10/>
                </div>
                <div>
                    <h2>"Open Positions"</h2>
                    <PositionTable/>
                </div>
            </div>
        </div>
    }
}
