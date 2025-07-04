# gate-fixtures

Test fixtures and sample data for Gate testing. Provides example requests/responses for various AI providers.

## Responsibilities

- **Test Data**: Sample API requests and responses
- **Provider Examples**: Real examples from OpenAI, Anthropic, etc.
- **Build-time Embedding**: Fixtures compiled into binary for tests
- **JSON Validation**: Ensures fixture validity

## Organization

```
fixtures/
   openai/          # OpenAI API examples
   anthropic/       # Anthropic API examples
   gate/            # Gate-specific test cases
```

## Usage

```rust
use gate_fixtures::{OPENAI_CHAT_REQUEST, ANTHROPIC_MESSAGE_RESPONSE};

#[test]
fn test_parsing() {
    let request: ChatCompletionRequest = 
        serde_json::from_str(OPENAI_CHAT_REQUEST)?;
    // Test with real-world data
}
```

## Build Process

`build.rs` walks fixture directories and embeds JSON files as string constants at compile time.

## Dependencies

Minimal:
- `serde_json`: JSON parsing validation
- `walkdir`: Build-time directory traversal

## Adding Fixtures

1. Add JSON file to appropriate provider directory
2. Rebuild crate to embed new fixture
3. Access via generated constant

## Risks

- **Size**: Large fixtures increase binary size
- **Maintenance**: Fixtures may become outdated as APIs evolve
- **Privacy**: Ensure no sensitive data in examples