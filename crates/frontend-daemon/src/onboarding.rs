use gate_frontend_common::components::Onboarding;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct OnboardingAuthProps {
    pub bootstrap_token: String,
}

#[function_component(OnboardingAuth)]
pub fn onboarding_auth(props: &OnboardingAuthProps) -> Html {
    // Handle onboarding completion
    let on_complete = {
        Callback::from(move |_| {
            // Redirect to home or refresh
            let window = web_sys::window().unwrap();
            let location = window.location();
            location.set_href("/").ok();
        })
    };

    // Use the new Onboarding component from frontend-common
    html! {
        <Onboarding
            bootstrap_token={props.bootstrap_token.clone()}
            on_complete={on_complete}
        />
    }
}
