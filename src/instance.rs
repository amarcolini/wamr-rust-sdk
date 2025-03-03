/*
 * Copyright (C) 2019 Intel Corporation. All rights reserved.
 * SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
 */

//! an instantiated module. The module is instantiated with the given imports.
//! get one via `Instance::new()`

#![allow(unused_variables)]

use alloc::string::String;
use core::{ffi::c_char, marker::PhantomData, ptr};
use core::ffi::c_void;
use wamr_sys::{wasm_memory_get_base_address, wasm_module_inst_t, wasm_runtime_deinstantiate, wasm_runtime_destroy_thread_env, wasm_runtime_get_app_addr_range, wasm_runtime_get_default_memory, wasm_runtime_get_native_addr_range, wasm_runtime_init_thread_env, wasm_runtime_instantiate};

use crate::{helper::error_buf_to_string, helper::DEFAULT_ERROR_BUF_SIZE, module::Module, runtime::Runtime, InstanceContext, RuntimeError};

#[derive(Debug)]
pub struct Instance<'module> {
    instance: wasm_module_inst_t,
    _phantom: PhantomData<Module<'module>>,
}

impl<'module> Instance<'module> {
    /// instantiate a module with stack size
    ///
    /// # Error
    ///
    /// Return `RuntimeError::CompilationError` if failed.
    pub fn new(
        runtime: &Runtime,
        module: &'module Module<'module>,
        stack_size: u32,
    ) -> Result<Self, RuntimeError> {
        Self::new_with_args(runtime, module, stack_size, 0)
    }

    /// instantiate a module with stack size and host managed heap size
    ///
    /// heap_size is used for `-nostdlib` Wasm and wasm32-unknown
    ///
    /// # Error
    ///
    /// Return `RuntimeError::CompilationError` if failed.
    pub fn new_with_args(
        _runtime: &Runtime,
        module: &'module Module<'module>,
        stack_size: u32,
        heap_size: u32,
    ) -> Result<Self, RuntimeError> {
        let init_thd_env = unsafe { wasm_runtime_init_thread_env() };
        if !init_thd_env {
            return Err(RuntimeError::InstantiationFailure(String::from(
                "thread signal env initialized failed",
            )));
        }

        let mut error_buf = [0 as c_char; DEFAULT_ERROR_BUF_SIZE];
        let instance = unsafe {
            wasm_runtime_instantiate(
                module.get_inner_module(),
                stack_size,
                heap_size,
                error_buf.as_mut_ptr(),
                error_buf.len() as u32,
            )
        };

        if instance.is_null() {
            match error_buf.len() {
                0 => {
                    return Err(RuntimeError::InstantiationFailure(String::from(
                        "instantiation failed",
                    )))
                }
                _ => {
                    return Err(RuntimeError::InstantiationFailure(error_buf_to_string(
                        &error_buf,
                    )))
                }
            }
        }

        Ok(Instance {
            instance,
            _phantom: PhantomData,
        })
    }

    pub fn get_inner_instance(&self) -> wasm_module_inst_t {
        self.instance
    }
}

impl Drop for Instance<'_> {
    fn drop(&mut self) {
        unsafe {
            wasm_runtime_destroy_thread_env();
            wasm_runtime_deinstantiate(self.instance);
        }
    }
}

pub(crate) struct InstanceRef<'a> {
    pub instance: wasm_module_inst_t,
    lifetime: PhantomData<&'a ()>,
}

impl<'a> InstanceRef<'a> {
    pub unsafe fn from_raw(instance: wasm_module_inst_t) -> Self {
        Self {
            instance,
            lifetime: PhantomData,
        }
    }

    /// Gets the base address and size of this instance's memory
    pub fn get_memory_range(&self) -> (*mut c_void, usize) {
        unsafe {
            let memory = wasm_runtime_get_default_memory(self.instance);
            assert!(!memory.is_null());
            let base_addr = wasm_memory_get_base_address(memory);
            let mut mem_size = 0;
            wasm_runtime_get_app_addr_range(self.instance, 0, ptr::null_mut(), &mut mem_size);
            (base_addr, mem_size as usize)
        }
    }
}

unsafe impl InstanceContext for Instance<'_> {
    fn as_instance_ref(&self) -> InstanceRef {
        unsafe { InstanceRef::from_raw(self.instance) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Runtime;
    use wamr_sys::{
        wasm_runtime_get_running_mode, RunningMode_Mode_Interp, RunningMode_Mode_LLVM_JIT,
    };

    #[test]
    fn test_instance_new() {
        let runtime = Runtime::new().unwrap();

        // (module
        //   (func (export "add") (param i32 i32) (result i32)
        //     (local.get 0)
        //     (local.get 1)
        //     (i32.add)
        //   )
        // )
        let binary = vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f,
            0x7f, 0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01, 0x03, 0x61, 0x64, 0x64,
            0x00, 0x00, 0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b,
        ];
        let binary = binary.into_iter().map(|c| c as u8).collect::<Vec<u8>>();

        let module = Module::from_vec(&runtime, binary, "add");
        assert!(module.is_ok());

        let module = &module.unwrap();

        let instance = Instance::new_with_args(&runtime, module, 1024, 1024);
        assert!(instance.is_ok());

        let instance = Instance::new_with_args(&runtime, module, 1024, 0);
        assert!(instance.is_ok());

        let instance = instance.unwrap();
        assert_eq!(
            unsafe { wasm_runtime_get_running_mode(instance.get_inner_instance()) },
            if cfg!(feature = "llvmjit") {
                RunningMode_Mode_LLVM_JIT
            } else {
                RunningMode_Mode_Interp
            }
        );
    }

    #[test]
    #[ignore]
    fn test_instance_running_mode_default() {
        let runtime = Runtime::builder().use_system_allocator().build().unwrap();

        // (module
        //   (func (export "add") (param i32 i32) (result i32)
        //     (local.get 0)
        //     (local.get 1)
        //     (i32.add)
        //   )
        // )
        let binary = vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f,
            0x7f, 0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01, 0x03, 0x61, 0x64, 0x64,
            0x00, 0x00, 0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b,
        ];
        let binary = binary.into_iter().map(|c| c as u8).collect::<Vec<u8>>();

        let module = Module::from_vec(&runtime, binary, "");
        assert!(module.is_ok());

        let module = &module.unwrap();

        let instance = Instance::new_with_args(&runtime, module, 1024, 1024);
        assert!(instance.is_ok());

        let instance = instance.unwrap();
        assert_eq!(
            unsafe { wasm_runtime_get_running_mode(instance.get_inner_instance()) },
            if cfg!(feature = "llvmjit") {
                RunningMode_Mode_LLVM_JIT
            } else {
                RunningMode_Mode_Interp
            }
        );
    }

    #[test]
    #[ignore]
    fn test_instance_running_mode_interpreter() {
        let runtime = Runtime::builder()
            .run_as_interpreter()
            .use_system_allocator()
            .build()
            .unwrap();

        // (module
        //   (func (export "add") (param i32 i32) (result i32)
        //     (local.get 0)
        //     (local.get 1)
        //     (i32.add)
        //   )
        // )
        let binary = vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f,
            0x7f, 0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01, 0x03, 0x61, 0x64, 0x64,
            0x00, 0x00, 0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b,
        ];
        let binary = binary.into_iter().map(|c| c as u8).collect::<Vec<u8>>();

        let module = Module::from_vec(&runtime, binary, "add");
        assert!(module.is_ok());

        let module = &module.unwrap();

        let instance = Instance::new_with_args(&runtime, module, 1024, 1024);
        assert!(instance.is_ok());

        let instance = instance.unwrap();
        assert_eq!(
            unsafe { wasm_runtime_get_running_mode(instance.get_inner_instance()) },
            RunningMode_Mode_Interp
        );
    }
}
