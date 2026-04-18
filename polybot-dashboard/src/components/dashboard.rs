use crate::components::{PositionTable, RiskSummary, SignalFeed};
use leptos::prelude::*;

#[component]
pub fn Dashboard() -> impl IntoView {
    view! {
        <div>
            <h1>"SuperFast PolyBot v3 Operator Panel"</h1>
            <p>"Calm, fast monitoring for the automated copy-trading runtime."</p>
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
