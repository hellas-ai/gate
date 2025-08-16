# Contributing to Gate

Thanks for your interest in improving Gate!

Your contributions will be made to the project under both the Apache 2.0 and
the MIT licenses.

We accept contributions in the form of:

- Pull Requests (via GitHub)
- Bug reports (via GitHub issues)
- User feedback (via [discord](https://discord.gg/YWZCWPrNTb))

We do **not** currently accept contributions for:

- Formatting (e.g., `cargo fmt`)
- Spelling and grammar

# Asking for help

If you'd like help, contact us on [discord](https://discord.gg/YWZCWPrNTb).

# Coding Guidelines

General guidelines:
- This is a production-grade codebase. No hacks, no shortcuts, no 'do this later'.
- This is a greenfield project. We want the highest-quality code possible, so if we need to refactor or change things, its better to do it sooner than later.
- Complexity is our enemy- we should strive to keep our codebase as simple as possible (but no simpler)
- We are free to change whatever we need in this project in order to achieve our goals

# Crate organisation
## Core Crate
This crate contains the core hellas data types, traits, etc.
- Needs to compile across wide range of platforms, including wasm32
- Any platform-specific dependencies (or stuff which doesn't compile for a target) MUST be feature-gated and disabled by default
- Feature-specific code (e.g tlsforward functionality) should NOT be added to this crate
- If at any point we find ourselves adding complexity, consider what we could refactor to make it better
- We should always leave codebase in a better state than we found it-
    - If we remove the last usage of a function argument, remove it from the function signature and fix callers
    - If we remove the last usage of a function/method- remove it

# Imports
- In general, there should never be more than two-levels of `::` nesting- e.g instead of using `std::sync::Arc<MyType..>`, add `use std::sync::Arc` and just use `Arc`.
- If we have a large number of imports from the same crate (common when loading e.g DTO types from client library):
    - Use wildcard imports (esp if upstream crate has a `::prelude` module):
        ```
        use some_client::types::*;
        fn accept(foo: FooType, bar: BarType) -> BazType;
        ```
    - Or module imports (should be used sparingly- `as ` should only be used when we have similarly-named type modules, to avoid confusion):
        ```
        use some_client_a::types as a_types;
        use some_client_b::types as b_types:
        fn convert(foo: a_types::Foo) -> b_types::Foo;
        ```
- If we need conditional dependencies (e,g for `wasm`, whatever platform):
    - Make a single conditional and put all the imports in it, don't make a new conditional for each import
- Put imports at the top of the file. We should not have import in e.g functions unless strictly necessary- ask my permission for this.

# Types
- We are very big of type safety. We should be using the type system as much as possible to help us.
- This means stuff like:
    - Deriving the `From/Into/TryInto/TryFrom` traits instead of custom conversion functions.
    - No "magic variables"- consts and strings etc should be defined at module level `const WHATEVER_NAME: &str = "whatever-thingy"`, if appropriate
    - Avoid unstructured data- we should perform ser/de at our system boundaries as soon as possible
    - Try to avoid `Option<>` as much as possible- lets think about whether we can refactor to remove ambiguity
    - Return structured errors, not Strings

# Style / Formatting
- This repo runs *strict* formatting and clippy checks. Don't worry so much about formatting as we can run `cargo fmt` before committing
- Clippy has lots of lints for unidiomatic code, try to add code that complies with these. e.g:
    - don't use `format!("my var: {}", var)` when you could use `format!("my var: {var}")`
    - no match blocks where if/else would suffice
    - appropriate use of `default` `self` etc.
- Try to write concise, terse code when possible. I don't mean doing everything on one line with one-character variables and semicolons- just use your common sense. 
- When fixing clippy lints, do NOT do 'lazy fixes'; do NOT 'ignore' them, by e.g:
    - renaming variables to start with `_my_unused_var`- we should just remove the symbol altogether.
    - adding #[allow(whatever)] - unless there is really good reason- ask me for explicit permission to do this

# Logging / Tracing
We use the `tracing` crate throughout this repo.
- Be very sparing with what is logged and at what level- noisy/spammy logs are worse than clean ones. If a code path is expected, it shouldn't be higher level than debug.
- We use OLTP library to export traces from e.g http servers. We should make new spans when necessary
- We must pass along correlation ids, trace parents etc, if present. For cross-service tracing.
- Adding explicit `use tracing::{info, warn};` etc imports cause a lot of code churn- `macro_use` them in crate root and skip importing.

# Comments
- This is a production-grade codebase, and as such documentation is very important.
- However, too many noisy/spamming/useless comments are bad too. They get out of date and are WORSE than useless.
- Do NOT leave comments like this:
    - `identity: LocalIdentity,  // Store identity in handle` - this is obvious, just don't write it
    - `identity: self.identity.clone(),  // Use stored identity` - again
    - Totally useless: ```
        // Update state
        self.state_tx.send(TlsForwardState::Connecting)?;
      ```
    - Confusing- why is it telling us about auth middleware?!: ```
        /// Create the configuration routes
        pub fn router() -> axum::Router<AppState<MinimalState>> {
            axum::Router::new().route("/api/config", routing::get(get_config).put(update_config))
            // Note: auth middleware is applied when converting to the final router
        }
        ```
- If a comment IS necessary, put it in standard rustdoc syntax. Don't go overboard with stuff like examples, defaults
- When refactoring code, feel free to drive-by cleanup any examples of these comments you see.

# Dependencies
- Relying too much on external libraries has various downsides- it bloats the code, may introduce security issues, forces strange api
- However we also don't want to re-invent the wheel and write too much code ourselves, and re-implementing tricky stuff like encryption has its own security issues
- In short, just be smart about adding dependencies. If you do need to add one:
    - Consider if it should be added as a workspace dependency- if more than one crate needs it
    - Should some other crate be re-exporting the dependency? For example, all our http code should be in `http` crate- probably that should re-export e.g `reqwest`
    - Be aware of features- when adding a crate, first check the available features. Probably we want to turn off the default features and just enable the ones we want
    - When adding a dependency to Cargo.toml, use coarse versions- e.g `thiserror = "1.0"`, `tracing = "0.2"`
    - If we remove the last remaining usage of a dependency in a crate, make sure to remove it from dependencies

# Error Handling
- This is production code, we should not have panic!(), unimplemented!(), todo!() anywhere- always return a Result.
    - In library code (ie, shared code that is used in different contexts):
        - Define a crate-level error type, using `thiserror`
        - Avoid using `.map_err` wherever possible- use the `#[from]` helper in `thiserror`
    - In application code (ie, code invoked directly by user, `main.rs`, etc)
        - It's fine to use `anyhow`
        - Add context where appropriate so backtraces, error messages are useful
- Try to avoid `map_err()` wherever possible- this means not only using `From` but `Into`- for example:
    - If you are writing HTTP handlers, your functions may return something like `impl IntoResponse`. We should implement
    `IntoResponse` (e,g map errors, to http status codes, format as json, give description)
    - If you are writing Tauri commands and they expect Result<String, String>, implement `Into<String>` for our errors

# Visibility
- We should think carefully about the visibility of all the code we write. Overuse of `pub` makes the library API enormous
and brittle, not to mention slows compiling/linking.
- If code is strictly heirachical, we can avoid making anything `pub` other than top-level crate exports. If not, we can use
e.g `pub(crate)`

# When embarking on a task
- Read ALL relevant information BEFORE starting to code. This includes the planning stage as well as implementation itself.
- Do not 'wait until i need to' to read existing code, do it proactively
- If you are at all unsure about the interface you're using, search for documentation, online if neccesary.
- If the interface is not behaving as you expect, read the documentation and fully understand whats different to what you expected, don't just bash away

# TODOs, Refactoring
- If during the process of implementing or planning some task, you see some beneficial refactoring that could be done, but is unrelated to ask in hand:
    - write a paragraph item to docs/REFACTOR_TODOS.md
    - this file should be treated as append-only- do not e.g update tasks in-progress here, add completed tasks etc.
    - dont start working on this refactor unless you are explicitly told to.
