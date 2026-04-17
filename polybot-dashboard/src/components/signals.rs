use crate::components::SignalFeed;
use leptos::prelude::*;

#[component]
pub fn Signals() -> impl IntoView {
    view! {
        <div>
            <h1>"Signal Feed"</h1>
            <SignalFeed limit=50/>
        </div>
    }
}
