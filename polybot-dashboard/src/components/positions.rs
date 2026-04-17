use crate::components::PositionTable;
use leptos::prelude::*;

#[component]
pub fn Positions() -> impl IntoView {
    view! {
        <div>
            <h1>"Open Positions"</h1>
            <PositionTable/>
        </div>
    }
}
