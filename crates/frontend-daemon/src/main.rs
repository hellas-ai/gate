mod components;
mod local_app;
mod local_auth;
mod onboarding;
mod services;
mod utils;

use local_app::LocalApp;
use onboarding::OnboardingAuth;
use yew::prelude::*;
use yew_router::prelude::*;

#[derive(Clone, Routable, PartialEq)]
enum Route {
    #[at("/")]
    Home,
    #[at("/bootstrap/:token")]
    Bootstrap { token: String },
}

fn switch(route: Route) -> Html {
    match route {
        Route::Home => html! { <LocalApp /> },
        Route::Bootstrap { token } => html! {
            <gate_frontend_common::theme::ThemeProvider>
                <gate_frontend_common::auth::AuthProvider>
                    <OnboardingAuth bootstrap_token={token} />
                </gate_frontend_common::auth::AuthProvider>
            </gate_frontend_common::theme::ThemeProvider>
        },
    }
}

#[function_component(App)]
fn app() -> Html {
    html! {
        <BrowserRouter>
            <Switch<Route> render={switch} />
        </BrowserRouter>
    }
}

fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<App>::new().render();
}
