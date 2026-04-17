use leptos::prelude::*;
use leptos_meta::*;

use crate::components::*;

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    let page: RwSignal<&'static str> = RwSignal::new("dashboard");
    let set_page = page;

    view! {
        <Stylesheet id="leptos" href="/style.css"/>
        <Title text="SuperFast PolyBot v2"/>
        <nav>
            <div>
                <span>"PolyBot v2"</span>
            </div>
            <div>
                <button on:click=move |_| set_page.set("dashboard")>"Dashboard"</button>
                <button on:click=move |_| set_page.set("positions")>"Positions"</button>
                <button on:click=move |_| set_page.set("signals")>"Signals"</button>
                <button on:click=move |_| set_page.set("risk")>"Risk"</button>
                <button on:click=move |_| set_page.set("health")>"Health"</button>
            </div>
        </nav>
        <main>
            {move || {
                let p = page.get();
                if p == "positions" {
                    view! { <Positions/> }.into_any()
                } else if p == "signals" {
                    view! { <Signals/> }.into_any()
                } else if p == "risk" {
                    view! { <RiskView/> }.into_any()
                } else if p == "health" {
                    view! { <HealthCheck/> }.into_any()
                } else {
                    view! { <Dashboard/> }.into_any()
                }
            }}
        </main>
    }
}
