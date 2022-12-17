//! Makes testing of REAPER extension plugins easy.
//!
//! For testing reaper extension, which itself is of type `cdylib`,
//! you need transform the project folder to workspace. So, basically,
//! project tree would look similar to this:
//!
//!```ignore
//! workspace_directory
//! ├── Cargo.toml
//! ├── README.md
//! ├── src
//! │   └── lib.rs
//! └── test
//!     ├── Cargo.toml
//!     ├── src
//!     │   └── lib.rs
//!     └── tests
//!         └── integration_test.rs
//!```
//!
//! `test` crate will not be delivered to the end-user, but will be used for
//! testing your library.
//!
//! Since there is a need for patching of reaper-low and
//! reaper-medium, contents of `test/Cargo.toml`:
//! ```ignore
//! [package]
//! edition = "2021"
//! name = "reaper-test-extension-plugin"
//! publish = false
//! version = "0.1.0"
//!
//! [dependencies]
//! reaper-low = "0.1.0"
//! reaper-macros = "0.1.0"
//! reaper-medium = "0.1.0"
//! reaper-test = "0.1.0"
//!
//! [patch.crates-io]
//! reaper-low = {git = "https://github.com/Levitanus/reaper-rs", branch = "stable_for_rea-rs"}
//! reaper-macros = {git = "https://github.com/Levitanus/reaper-rs", branch = "stable_for_rea-rs"}
//! reaper-medium = {git = "https://github.com/Levitanus/reaper-rs", branch = "stable_for_rea-rs"}
//! reaper-test = {git = "https://github.com/Levitanus/reaper-test"}
//!
//! [lib]
//! crate-type = ["cdylib"]
//! name = "reaper_test_extension_plugin"
//! ```
//!
//! contents of `test/tests/integration_test.rs`:
//! ```ignore
//! use reaper_test::{run_integration_test, ReaperVersion};
//!
//! #[test]
//! fn main() {
//!     run_integration_test(ReaperVersion::latest());
//! }
//! ```
//!
//! `test/src/lib.rs` is the file your integration tests are placed in.
//! ```ignore
//! use reaper_macros::reaper_extension_plugin;
//! use reaper_test::*;
//! use std::error::Error;
//!
//! fn hello_world(reaper: &ReaperTest) -> TestStepResult {
//!     reaper.medium().show_console_msg("Hello world!");
//!     Ok(())
//! }
//!
//! #[reaper_extension_plugin]
//! fn test_extension(context: PluginContext) -> Result<(), Box<dyn Error>> {
//!     // setup test global environment
//!     let test = ReaperTest::setup(context, "test_action");
//!     // Push single test step.
//!     test.push_test_step(TestStep::new("Hello World!", hello_world));
//!     Ok(())
//! }
//! ```
//!
//! to run integration tests, go to the test folder and type:
//! `cargo build --workspace; cargo test`
//!

use reaper_low::register_plugin_destroy_hook;
use reaper_medium::{CommandId, ControlSurface, HookCommand, OwnedGaccelRegister};
use std::{error::Error, fmt::Debug, panic, process};

pub mod integration_test;
pub use integration_test::*;
pub use reaper_low::PluginContext;

static mut INSTANCE: Option<ReaperTest> = None;

pub type TestStepResult = Result<(), Box<dyn Error>>;
pub type TestCallback = dyn Fn(&'static ReaperTest) -> TestStepResult;

pub struct TestStep {
    name: String,
    operation: Box<TestCallback>,
}
impl TestStep {
    pub fn new(
        name: impl Into<String>,
        operation: impl Fn(&'static ReaperTest) -> Result<(), Box<dyn Error>> + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            operation: Box::new(operation),
        }
    }
}
impl Debug for TestStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Debug)]
struct ActionHook {
    actions: Vec<CommandId>,
}
impl ActionHook {
    fn new() -> Self {
        return Self {
            actions: Vec::new(),
        };
    }
}
impl HookCommand for ActionHook {
    fn call(command_id: reaper_medium::CommandId, _flag: i32) -> bool {
        let rpr = ReaperTest::get_mut();
        let hook = rpr.action_hook.as_ref().expect("should be hook here");
        for action in hook.actions.iter() {
            if action.get() == command_id.get() {
                rpr.test();
                return true;
            }
        }
        return false;
    }
}

#[derive(Debug)]
pub struct ReaperTest {
    low: reaper_low::Reaper,
    medium_session: reaper_medium::ReaperSession,
    medium: reaper_medium::Reaper,
    action_hook: Option<ActionHook>,
    steps: Vec<TestStep>,
    is_integration_test: bool,
}
impl ReaperTest {
    /// Makes the given instance available globally.
    ///
    /// After this has been called, the instance can be queried globally using
    /// `get()`.
    ///
    /// This can be called once only. Subsequent calls won't have any effect!
    fn make_available_globally(reaper: ReaperTest) {
        static INIT_INSTANCE: std::sync::Once = std::sync::Once::new();
        unsafe {
            INIT_INSTANCE.call_once(|| {
                INSTANCE = Some(reaper);
                register_plugin_destroy_hook(|| INSTANCE = None);
            });
        }
    }

    pub fn setup(context: PluginContext, action_name: &'static str) -> &'static mut Self {
        let low = reaper_low::Reaper::load(context);
        let medium_session = reaper_medium::ReaperSession::new(low);
        let medium = medium_session.reaper().clone();
        reaper_medium::Reaper::make_available_globally(medium.clone());
        let mut instance = Self {
            low,
            medium_session,
            medium,
            action_hook: None,
            steps: Vec::new(),
            is_integration_test: std::env::var("RUN_REAPER_INTEGRATION_TEST").is_ok(),
        };
        let integration = instance.is_integration_test;
        instance.register_action(action_name, action_name);
        Self::make_available_globally(instance);
        let obj = ReaperTest::get_mut();
        if integration {
            obj.medium_session_mut()
                .plugin_register_add_csurf_inst(Box::new(ReaperTestSurface {}))
                .expect("Can not register test control surface");
        }
        ReaperTest::get_mut()
    }
    pub fn low(&self) -> &reaper_low::Reaper {
        &self.low
    }
    pub fn medium_session(&self) -> &reaper_medium::ReaperSession {
        &self.medium_session
    }
    pub fn medium_session_mut(&mut self) -> &mut reaper_medium::ReaperSession {
        &mut self.medium_session
    }
    pub fn medium(&self) -> &reaper_medium::Reaper {
        &self.medium
    }

    /// Gives access to the instance which you made available globally before.
    ///
    /// # Panics
    ///
    /// This panics if [`make_available_globally()`] has not been called
    /// before.
    ///
    /// [`make_available_globally()`]: fn.make_available_globally.html
    pub fn get() -> &'static ReaperTest {
        unsafe {
            INSTANCE
                .as_ref()
                .expect("call `load(context)` before using `get()`")
        }
    }
    pub fn get_mut() -> &'static mut ReaperTest {
        unsafe {
            INSTANCE
                .as_mut()
                .expect("call `load(context)` before using `get()`")
        }
    }

    fn test(&mut self) {
        println!("# Testing reaper-rs\n");
        let result = panic::catch_unwind(|| -> TestStepResult {
            let rpr = ReaperTest::get();
            for step in rpr.steps.iter() {
                println!("Testing step: {}", step.name);
                (step.operation)(rpr)?;
            }
            Ok(())
        });
        let final_result = match result.is_err() {
            false => result.unwrap(),
            true => Err("Reaper panicked!".into()),
        };
        match final_result {
            Ok(_) => {
                println!("From REAPER: reaper-rs integration test executed successfully");
                if self.is_integration_test {
                    process::exit(0)
                }
            }
            Err(reason) => {
                // We use a particular exit code to distinguish test
                // failure from other possible
                // exit paths.
                match self.is_integration_test {
                    true => {
                        eprintln!("From REAPER: reaper-rs integration test failed: {}", reason);
                        process::exit(172)
                    }
                    false => panic!("From REAPER: reaper-rs integration test failed: {}", reason),
                }
            }
        }
    }

    pub fn push_test_step(&mut self, step: TestStep) {
        self.steps.push(step);
    }

    fn register_action(
        &mut self,
        command_name: &'static str,
        description: &'static str,
    ) -> CommandId {
        self.check_action_hook();
        let hook = self.action_hook.as_mut().expect("should be hook here");
        let medium = &mut self.medium_session;
        let command_id = medium.plugin_register_add_command_id(command_name).unwrap();
        medium
            .plugin_register_add_gaccel(OwnedGaccelRegister::without_key_binding(
                command_id,
                description,
            ))
            .expect("Can not register test action");
        let command_id = CommandId::from(command_id);
        hook.actions.push(command_id);
        command_id
    }

    fn check_action_hook(&mut self) {
        if self.action_hook.is_none() {
            self.action_hook = Some(ActionHook::new());
            self.medium_session
                .plugin_register_add_hook_command::<ActionHook>()
                .expect("can not register hook");
        }
    }
}

#[derive(Debug)]
struct ReaperTestSurface {}
impl ControlSurface for ReaperTestSurface {
    fn run(&mut self) {
        let rpr = ReaperTest::get_mut();
        if rpr.is_integration_test {
            rpr.test();
            rpr.is_integration_test = false;
        }
    }
}
