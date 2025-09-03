use wasm_bindgen::JsCast;
use web_sys::window;

pub fn update_favicon(is_dark_mode: bool) {
    if let Some(window) = window() {
        if let Some(document) = window.document() {
            // Use white icon in dark mode, black icon in light mode for better contrast
            let favicon_path = if is_dark_mode {
                "/assets/hellas-token-white.svg"
            } else {
                "/assets/hellas-token-black.svg"
            };

            // Remove existing favicon links
            let links = document.query_selector_all("link[rel*='icon']").unwrap();
            for i in 0..links.length() {
                if let Some(link) = links.item(i) {
                    if let Ok(element) = link.dyn_into::<web_sys::Element>() {
                        element.remove();
                    }
                }
            }

            // Create new favicon link
            if let Some(head) = document.head() {
                if let Ok(link) = document.create_element("link") {
                    let link = link.dyn_into::<web_sys::HtmlLinkElement>().unwrap();
                    link.set_rel("icon");
                    link.set_type("image/svg+xml");
                    link.set_href(favicon_path);
                    let _ = head.append_child(&link);
                }
            }
        }
    }
}

pub fn init_favicon_observer() {
    // Check for theme on initial load
    let is_dark = is_dark_mode();
    update_favicon(is_dark);

    // Watch for theme changes via class on html element
    if let Some(window) = window() {
        use wasm_bindgen::closure::Closure;

        let callback = Closure::wrap(Box::new(move || {
            let is_dark = is_dark_mode();
            update_favicon(is_dark);
        }) as Box<dyn Fn()>);

        // Use a simple interval as MutationObserver is complex in wasm
        let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            1000, // Check every second
        );
        callback.forget();
    }
}

fn is_dark_mode() -> bool {
    if let Some(window) = window() {
        if let Some(document) = window.document() {
            if let Some(html_element) = document.document_element() {
                return html_element.class_list().contains("dark");
            }
        }
    }
    false
}
