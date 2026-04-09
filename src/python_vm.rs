use rustpython_vm as vm;
use rustpython_pylib;

pub struct PythonExecutor;

impl PythonExecutor {
    pub fn new() -> Self {
        Self
    }

    pub fn execute(&self, source: &str) -> Result<String, String> {
        let interpreter = vm::Interpreter::with_init(Default::default(), |vm| {
            vm.add_frozen(rustpython_pylib::FROZEN_STDLIB);
        });

        interpreter.enter(|vm| {
            let scope = vm.new_scope_with_builtins();
            
            let code = vm.compile(source, vm::compiler::Mode::Exec, "<embedded>".to_owned())
                .map_err(|e| format!("Python Compile Error: {}", e))?;
            
            let _ = vm.run_code_obj(code, scope)
                .map_err(|e| {
                    let mut s = String::new();
                    vm.write_exception(&mut s, &e).unwrap();
                    format!("Python Runtime Error: {}", s)
                })?;
            
            Ok("Execution successful".to_string())
        })
    }
    
    /// Execute a specific function by name and return string result
    pub fn call_function(&self, script: &str, func_name: &str, target: &str, task: &str) -> Result<String, String> {
        let interpreter = vm::Interpreter::with_init(Default::default(), |vm| {
            vm.add_frozen(rustpython_pylib::FROZEN_STDLIB);
        });
        
        interpreter.enter(|vm| {
            let scope = vm.new_scope_with_builtins();
            
            // Load script
            let code = vm.compile(script, vm::compiler::Mode::Exec, "<script>".to_owned())
                .map_err(|e| format!("Python Load Error: {}", e))?;
            vm.run_code_obj(code, scope.clone()).map_err(|e| format!("Python Init Error: {:?}", e))?;
            
            // Get function handle
            let func = scope.locals.get_item(func_name, vm)
                .map_err(|_| format!("Function {} not found in script", func_name))?;
                
            // Call function
            let args: Vec<vm::PyObjectRef> = vec![
                vm.ctx.new_str(target).into(), 
                vm.ctx.new_str(task).into()
            ];
            
            // In 0.4.0, invoke is deprecated in favor of func.call(args, vm)
            let result = func.call(args, vm)
                .map_err(|e| {
                    let mut s = String::new();
                    vm.write_exception(&mut s, &e).unwrap();
                    format!("Python Invoke Error: {}", s)
                })?;
                
            // Convert result to string
            let py_str = result.str(vm).map_err(|e| format!("Result to str error: {:?}", e))?;
            Ok(py_str.as_str().to_string())
        })
    }
}
