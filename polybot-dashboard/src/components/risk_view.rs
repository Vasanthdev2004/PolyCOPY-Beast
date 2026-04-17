use crate::components::RiskSummary;
use leptos::prelude::*;

#[component]
pub fn RiskView() -> impl IntoView {
    view! {
        <div>
            <h1>"Risk Dashboard"</h1>
            <RiskSummary/>
            <div>
                <h2>"Exposure Limits"</h2>
                <p>"Daily max loss: 5%"</p>
                <p>"Per-market exposure: 10%"</p>
                <p>"Per-category exposure: 25%"</p>
                <p>"Max position size: $500"</p>
                <p>"Min confidence: 0.60"</p>
            </div>
            <div>
                <h2>"Category Allocation"</h2>
                <p>"Politics: 25% max"</p>
                <p>"Sports: 20% max"</p>
                <p>"Crypto: 15% max"</p>
                <p>"Others: 10% max"</p>
            </div>
        </div>
    }
}
