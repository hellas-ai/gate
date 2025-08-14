# gate-chat-ui

Reusable chat interface component for Yew applications. Used by gate-frontend-daemon and other frontends to provide the chat testing interface.

## Responsibilities

- **Chat Component**: Standalone Yew component for AI chat
- **Message Rendering**: Markdown support with syntax highlighting
- **Streaming Support**: Real-time token streaming display
- **Auto-resize**: Smart textarea that grows with content
- **Keyboard Shortcuts**: Enter to send, Shift+Enter for newline

## Usage

```rust
use gate_chat_ui::ChatInterface;
use yew::prelude::*;

#[function_component(App)]
fn app() -> Html {
    let on_send = Callback::from(|message: String| {
        // Handle message sending
    });

    html! {
        <ChatInterface 
            on_send={on_send}
            placeholder="Type a message..."
            initial_messages={vec![]}
        />
    }
}
```

## Features

- Message history with sender attribution
- Loading states during inference
- Error message display
- Copy code blocks to clipboard
- Auto-scroll to latest message
- Mobile-responsive design

## Component Props

- `on_send`: Callback when user sends message
- `placeholder`: Input field placeholder text
- `initial_messages`: Pre-populate chat history
- `disabled`: Disable input during processing

## Dependencies

Minimal WASM-compatible:
- `yew`: Component framework
- `web-sys`: DOM manipulation
- `gloo-timers`: Async timeouts
- `js-sys`: JavaScript interop

## Styling

Uses Tailwind CSS classes. Expects Tailwind to be available in parent application.

## Browser Requirements

- Modern browsers with ES2015+ support
- ResizeObserver API for auto-resize
- Clipboard API for copy functionality

## Risks

- **Parent Styling**: Requires Tailwind CSS in parent app
- **Browser APIs**: Some features need polyfills for older browsers
- **Memory**: Long conversations increase memory usage