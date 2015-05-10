/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this file,
 * You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Rust wrappers around the raw JS apis

use libc::types::os::arch::c95::{size_t, c_uint};
use libc::c_char;
use std::ffi;
use std::rc;
use std::str;
use std::u32;
use jsapi::*;
use jsapi::JSVersion::JSVERSION_LATEST;
use jsval::{JSVal, NullValue};
use default_stacksize;
use default_heapsize;
use JSOPTION_VAROBJFIX;
use JSOPTION_METHODJIT;
use JSOPTION_TYPE_INFERENCE;
use JSOPTION_DONT_REPORT_UNCAUGHT;
use JSOPTION_AUTOJSAPI_OWNS_ERROR_REPORTING;
use ERR;

// ___________________________________________________________________________
// friendly Rustic API to runtimes

/// A wrapper for the `JSRuntime` and `JSContext` structures in SpiderMonkey.
pub struct Runtime {
    pub rt: rt,
    pub cx: rc::Rc<Cx>,
}

impl Runtime {
    /// Creates a new `JSRuntime` and `JSContext`.
    pub fn new() -> Runtime {
        let js_runtime = unsafe { JS_Init(default_heapsize) };
        assert!(!js_runtime.is_null());

        // Unconstrain the runtime's threshold on nominal heap size, to avoid
        // triggering GC too often if operating continuously near an arbitrary
        // finite threshold. This leaves the maximum-JS_malloc-bytes threshold
        // still in effect to cause periodical, and we hope hygienic,
        // last-ditch GCs from within the GC's allocator.
        unsafe {
            JS_SetGCParameter(js_runtime, JSGC_MAX_BYTES, u32::MAX);
        }

        let js_context = unsafe {
            JS_NewContext(js_runtime, default_stacksize as size_t)
        };
        assert!(!js_context.is_null());

        unsafe {
            JS_SetOptions(js_context,
                          JSOPTION_VAROBJFIX |
                          JSOPTION_METHODJIT |
                          JSOPTION_TYPE_INFERENCE |
                          JSOPTION_DONT_REPORT_UNCAUGHT |
                          JSOPTION_AUTOJSAPI_OWNS_ERROR_REPORTING);

            JS_SetVersion(js_context, JSVERSION_LATEST);
            JS_SetErrorReporter(js_context,
                                Some(reportError as unsafe extern "C"
                                     fn(*mut JSContext, *const c_char, *mut JSErrorReport)));
            JS_SetGCZeal(js_context, 0, JS_DEFAULT_ZEAL_FREQ);
        }

        let js_runtime = rc::Rc::new(rt_rsrc {
            ptr: js_runtime
        });
        let js_context = rc::Rc::new(Cx {
            ptr: js_context,
            rt: js_runtime.clone(),
        });
        Runtime {
            rt: js_runtime,
            cx: js_context,
        }
    }

    /// Returns the `JSRuntime` object.
    pub fn rt(&self) -> *mut JSRuntime {
        self.rt.ptr
    }

    /// Returns the `JSContext` object.
    pub fn cx(&self) -> *mut JSContext {
        self.cx.ptr
    }
}

pub type rt = rc::Rc<rt_rsrc>;

pub struct rt_rsrc {
    pub ptr : *mut JSRuntime,
}

impl Drop for rt_rsrc {
    fn drop(&mut self) {
        unsafe {
            JS_Finish(self.ptr);
        }
    }
}

// ___________________________________________________________________________
// contexts

pub struct Cx {
    pub ptr: *mut JSContext,
    pub rt: rt,
}

impl Drop for Cx {
    fn drop(&mut self) {
        unsafe {
            JS_DestroyContext(self.ptr);
        }
    }
}

impl Cx {
    pub fn evaluate_script(&self, glob: *mut JSObject, script: String, filename: String, line_num: usize)
                    -> Result<(),()> {
        let script_utf16: Vec<u16> = script.utf16_units().collect();
        let filename_cstr = ffi::CString::new(filename.as_bytes()).unwrap();
        let mut rval: JSVal = NullValue();
        debug!("Evaluating script from {} with content {}", filename, script);
        // SpiderMonkey does not approve of null pointers.
        let (ptr, len) = if script_utf16.len() == 0 {
            static empty: &'static [u16] = &[];
            (empty.as_ptr(), 0)
        } else {
            (script_utf16.as_ptr(), script_utf16.len() as c_uint)
        };
        assert!(!ptr.is_null());
        unsafe {
            if ERR == JS_EvaluateUCScript(self.ptr, glob, ptr, len,
                                          filename_cstr.as_ptr(), line_num as c_uint,
                                          &mut rval) {
                debug!("...err!");
                Err(())
            } else {
                // we could return the script result but then we'd have
                // to root it and so forth and, really, who cares?
                debug!("...ok!");
                Ok(())
            }
        }
    }
}

pub unsafe extern fn reportError(_cx: *mut JSContext, msg: *const c_char, report: *mut JSErrorReport) {
    let fnptr = (*report).filename;
    let fname = if !fnptr.is_null() {
        let c_str = ffi::CStr::from_ptr(fnptr);
        str::from_utf8(c_str.to_bytes()).ok().unwrap().to_string()
    } else {
        "none".to_string()
    };
    let lineno = (*report).lineno;
    let c_str = ffi::CStr::from_ptr(msg);
    let msg = str::from_utf8(c_str.to_bytes()).ok().unwrap().to_string();
    error!("Error at {}:{}: {}\n", fname, lineno, msg);
}

pub fn with_compartment<R, F: FnMut() -> R>(cx: *mut JSContext, object: *mut JSObject, mut cb: F) -> R {
    unsafe {
        let call = JS_EnterCrossCompartmentCall(cx, object);
        let result = cb();
        JS_LeaveCrossCompartmentCall(call);
        result
    }
}

#[cfg(test)]
pub mod test {
    use super::Runtime;

    #[test]
    pub fn dummy() {
        let _rt = Runtime::new();
    }

}
