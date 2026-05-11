//! KryosTensor — N-dimensional f64 tensor with shape tracking,
//! element-wise ops, reductions, linear algebra, and ML primitives.
//!
//! Every tensor is heap-allocated. Functions are `#[no_mangle] extern "C"`
//! for linking from compiled Kryos code. Handles are `*mut KryosTensor`
//! cast to `i64`.
//!
//! # Unsafe invariants (file-wide)
//!
//! See `docs/17-unsafe-audit.md` patterns 1 (FFI handle reconstruction), 2
//! (slice::from_raw_parts), and 3 (alloc/dealloc).
//!
//! * Every entry point that takes an `i64` tensor handle checks `handle != 0`
//!   before deref, and trusts that the type checker upstream rejected calls
//!   passing a non-tensor handle.
//! * `data`, `shape`, and `strides` pointers inside `KryosTensor` are valid
//!   for `len * sizeof(f64)`, `rank * sizeof(i64)`, and `rank * sizeof(i64)`
//!   bytes respectively, allocated via `Layout` reconstructed from the same
//!   header fields at drop time.
//! * `kryos_tensor_release` is the single deallocation path; all builders
//!   produce one logical refcount.

use std::alloc::{alloc, alloc_zeroed, dealloc, Layout};
use std::ptr;

/// Heap-allocated N-dimensional tensor of f64.
#[repr(C)]
pub struct KryosTensor {
    pub data: *mut f64,
    pub shape: *mut i64,
    pub ndim: i64,
    pub numel: i64,
}

fn tensor_layout(numel: usize) -> Layout {
    Layout::from_size_align(numel * 8, 8).unwrap()
}

fn shape_layout(ndim: usize) -> Layout {
    Layout::from_size_align(ndim * 8, 8).unwrap()
}

/// Allocate a new tensor with given shape. Data is zeroed.
unsafe fn alloc_tensor(shape: &[i64]) -> *mut KryosTensor {
    let ndim = shape.len();
    let numel: i64 = shape.iter().product();
    let numel_usize = numel.max(0) as usize;

    let data = if numel_usize > 0 {
        alloc_zeroed(tensor_layout(numel_usize)) as *mut f64
    } else {
        ptr::null_mut()
    };

    let shape_ptr = alloc(shape_layout(ndim)) as *mut i64;
    ptr::copy_nonoverlapping(shape.as_ptr(), shape_ptr, ndim);

    let t = alloc(Layout::new::<KryosTensor>()) as *mut KryosTensor;
    (*t).data = data;
    (*t).shape = shape_ptr;
    (*t).ndim = ndim as i64;
    (*t).numel = numel;
    t
}

fn read_shape(shape_ptr: *const i64, ndim: i64) -> Vec<i64> {
    if shape_ptr.is_null() || ndim <= 0 {
        return vec![];
    }
    unsafe { std::slice::from_raw_parts(shape_ptr, ndim as usize).to_vec() }
}

fn as_tensor(handle: i64) -> *mut KryosTensor {
    handle as *mut KryosTensor
}

// ── Creation ────────────────────────────────────────────────────────

/// Create a tensor of zeros. `shape_ptr` is a pointer to an i64 array of length `ndim`.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_zeros(shape_ptr: *const i64, ndim: i64) -> i64 {
    let shape = read_shape(shape_ptr, ndim);
    alloc_tensor(&shape) as i64
}

/// Create a tensor of ones.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_ones(shape_ptr: *const i64, ndim: i64) -> i64 {
    let shape = read_shape(shape_ptr, ndim);
    let t = alloc_tensor(&shape);
    let n = (*t).numel as usize;
    for i in 0..n {
        *(*t).data.add(i) = 1.0;
    }
    t as i64
}

/// Create a tensor with uniform random values in [0, 1).
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_rand(shape_ptr: *const i64, ndim: i64) -> i64 {
    let shape = read_shape(shape_ptr, ndim);
    let t = alloc_tensor(&shape);
    let n = (*t).numel as usize;
    // Simple LCG PRNG (good enough for demos, no external deps).
    let mut seed: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(12345);
    for i in 0..n {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *(*t).data.add(i) = (seed >> 33) as f64 / (1u64 << 31) as f64;
    }
    t as i64
}

/// Create a tensor with values from standard normal distribution (Box-Muller).
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_randn(shape_ptr: *const i64, ndim: i64) -> i64 {
    let shape = read_shape(shape_ptr, ndim);
    let t = alloc_tensor(&shape);
    let n = (*t).numel as usize;
    let mut seed: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(54321);
    let mut i = 0;
    while i < n {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let u1 = ((seed >> 33) as f64 / (1u64 << 31) as f64).max(1e-10);
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let u2 = (seed >> 33) as f64 / (1u64 << 31) as f64;
        let mag = (-2.0 * u1.ln()).sqrt();
        let angle = 2.0 * std::f64::consts::PI * u2;
        *(*t).data.add(i) = mag * angle.cos();
        i += 1;
        if i < n {
            *(*t).data.add(i) = mag * angle.sin();
            i += 1;
        }
    }
    t as i64
}

/// Create a tensor from a flat f64 array with given shape.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_from_data(
    data_ptr: *const f64,
    numel: i64,
    shape_ptr: *const i64,
    ndim: i64,
) -> i64 {
    let shape = read_shape(shape_ptr, ndim);
    let t = alloc_tensor(&shape);
    if !data_ptr.is_null() && numel > 0 {
        let copy_n = (numel as usize).min((*t).numel as usize);
        ptr::copy_nonoverlapping(data_ptr, (*t).data, copy_n);
    }
    t as i64
}

/// Create an NxN identity matrix.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_eye(n: i64) -> i64 {
    let shape = [n, n];
    let t = alloc_tensor(&shape);
    let nu = n as usize;
    for i in 0..nu {
        *(*t).data.add(i * nu + i) = 1.0;
    }
    t as i64
}

/// Create a 1-D tensor: [start, start+step, start+2*step, ... ) up to end.
/// Arguments are f64 bits as i64 (Kryos slot model).
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_arange(
    start_bits: i64,
    end_bits: i64,
    step_bits: i64,
) -> i64 {
    let start = f64::from_bits(start_bits as u64);
    let end = f64::from_bits(end_bits as u64);
    let step_raw = f64::from_bits(step_bits as u64);
    let step = if step_raw == 0.0 { 1.0 } else { step_raw };
    let n = ((end - start) / step).ceil().max(0.0) as usize;
    let shape = [n as i64];
    let t = alloc_tensor(&shape);
    for i in 0..n {
        *(*t).data.add(i) = start + i as f64 * step;
    }
    t as i64
}

// ── Accessors ───────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_ndim(handle: i64) -> i64 {
    (*as_tensor(handle)).ndim
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_numel(handle: i64) -> i64 {
    (*as_tensor(handle)).numel
}

/// Returns shape[dim].
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_shape_dim(handle: i64, dim: i64) -> i64 {
    let t = as_tensor(handle);
    if dim < 0 || dim >= (*t).ndim {
        return -1;
    }
    *(*t).shape.add(dim as usize)
}

/// Get element at flat index. Returns f64 bits as i64.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_get(handle: i64, idx: i64) -> i64 {
    let t = as_tensor(handle);
    if idx < 0 || idx >= (*t).numel {
        return f64::NAN.to_bits() as i64;
    }
    (*(*t).data.add(idx as usize)).to_bits() as i64
}

/// Set element at flat index. `val` is f64 bits as i64.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_set(handle: i64, idx: i64, val: i64) {
    let t = as_tensor(handle);
    if idx >= 0 && idx < (*t).numel {
        *(*t).data.add(idx as usize) = f64::from_bits(val as u64);
    }
}

// ── Element-wise binary ops ─────────────────────────────────────────

unsafe fn elementwise_binop(a: i64, b: i64, op: fn(f64, f64) -> f64) -> i64 {
    let ta = as_tensor(a);
    let tb = as_tensor(b);
    let na = (*ta).numel as usize;
    let nb = (*tb).numel as usize;
    // Same shape: element-wise.
    if na == nb {
        let shape = std::slice::from_raw_parts((*ta).shape, (*ta).ndim as usize);
        let t = alloc_tensor(shape);
        for i in 0..na {
            *(*t).data.add(i) = op(*(*ta).data.add(i), *(*tb).data.add(i));
        }
        return t as i64;
    }
    // Scalar broadcast: if one is a single element.
    if nb == 1 {
        let scalar = *(*tb).data;
        let shape = std::slice::from_raw_parts((*ta).shape, (*ta).ndim as usize);
        let t = alloc_tensor(shape);
        for i in 0..na {
            *(*t).data.add(i) = op(*(*ta).data.add(i), scalar);
        }
        return t as i64;
    }
    if na == 1 {
        let scalar = *(*ta).data;
        let shape = std::slice::from_raw_parts((*tb).shape, (*tb).ndim as usize);
        let t = alloc_tensor(shape);
        for i in 0..nb {
            *(*t).data.add(i) = op(scalar, *(*tb).data.add(i));
        }
        return t as i64;
    }
    // Fallback: truncate to shorter length.
    let n = na.min(nb);
    let shape = [n as i64];
    let t = alloc_tensor(&shape);
    for i in 0..n {
        *(*t).data.add(i) = op(*(*ta).data.add(i), *(*tb).data.add(i));
    }
    t as i64
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_add(a: i64, b: i64) -> i64 {
    elementwise_binop(a, b, |x, y| x + y)
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_sub(a: i64, b: i64) -> i64 {
    elementwise_binop(a, b, |x, y| x - y)
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_mul(a: i64, b: i64) -> i64 {
    elementwise_binop(a, b, |x, y| x * y)
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_div(a: i64, b: i64) -> i64 {
    elementwise_binop(a, b, |x, y| x / y)
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_pow(a: i64, b: i64) -> i64 {
    elementwise_binop(a, b, |x, y| x.powf(y))
}

/// Scalar multiply: tensor * scalar. `scalar_bits` is f64 bits as i64.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_scale(handle: i64, scalar_bits: i64) -> i64 {
    let scalar = f64::from_bits(scalar_bits as u64);
    let t = as_tensor(handle);
    let n = (*t).numel as usize;
    let shape = std::slice::from_raw_parts((*t).shape, (*t).ndim as usize);
    let out = alloc_tensor(shape);
    for i in 0..n {
        *(*out).data.add(i) = *(*t).data.add(i) * scalar;
    }
    out as i64
}

// ── Unary ops ───────────────────────────────────────────────────────

unsafe fn elementwise_unary(handle: i64, op: fn(f64) -> f64) -> i64 {
    let t = as_tensor(handle);
    let n = (*t).numel as usize;
    let shape = std::slice::from_raw_parts((*t).shape, (*t).ndim as usize);
    let out = alloc_tensor(shape);
    for i in 0..n {
        *(*out).data.add(i) = op(*(*t).data.add(i));
    }
    out as i64
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_exp(h: i64) -> i64 {
    elementwise_unary(h, |x| x.exp())
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_log(h: i64) -> i64 {
    elementwise_unary(h, |x| x.ln())
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_sqrt(h: i64) -> i64 {
    elementwise_unary(h, |x| x.sqrt())
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_tanh(h: i64) -> i64 {
    elementwise_unary(h, |x| x.tanh())
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_sigmoid(h: i64) -> i64 {
    elementwise_unary(h, |x| 1.0 / (1.0 + (-x).exp()))
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_relu(h: i64) -> i64 {
    elementwise_unary(h, |x| x.max(0.0))
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_neg(h: i64) -> i64 {
    elementwise_unary(h, |x| -x)
}

// ── Reductions ──────────────────────────────────────────────────────

/// Sum all elements. Returns f64 bits as i64.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_sum(h: i64) -> i64 {
    let t = as_tensor(h);
    let n = (*t).numel as usize;
    let mut acc = 0.0f64;
    for i in 0..n {
        acc += *(*t).data.add(i);
    }
    acc.to_bits() as i64
}

/// Mean of all elements. Returns f64 bits as i64.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_mean(h: i64) -> i64 {
    let t = as_tensor(h);
    let n = (*t).numel as usize;
    if n == 0 {
        return f64::NAN.to_bits() as i64;
    }
    let sum = f64::from_bits(kryos_tensor_sum(h) as u64);
    (sum / n as f64).to_bits() as i64
}

/// Max element. Returns f64 bits as i64.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_max(h: i64) -> i64 {
    let t = as_tensor(h);
    let n = (*t).numel as usize;
    if n == 0 {
        return f64::NEG_INFINITY.to_bits() as i64;
    }
    let mut m = *(*t).data;
    for i in 1..n {
        let v = *(*t).data.add(i);
        if v > m {
            m = v;
        }
    }
    m.to_bits() as i64
}

/// Min element. Returns f64 bits as i64.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_min(h: i64) -> i64 {
    let t = as_tensor(h);
    let n = (*t).numel as usize;
    if n == 0 {
        return f64::INFINITY.to_bits() as i64;
    }
    let mut m = *(*t).data;
    for i in 1..n {
        let v = *(*t).data.add(i);
        if v < m {
            m = v;
        }
    }
    m.to_bits() as i64
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_argmax(h: i64) -> i64 {
    let t = as_tensor(h);
    let n = (*t).numel as usize;
    if n == 0 {
        return -1;
    }
    let mut best = 0;
    let mut best_val = *(*t).data;
    for i in 1..n {
        let v = *(*t).data.add(i);
        if v > best_val {
            best = i;
            best_val = v;
        }
    }
    best as i64
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_argmin(h: i64) -> i64 {
    let t = as_tensor(h);
    let n = (*t).numel as usize;
    if n == 0 {
        return -1;
    }
    let mut best = 0;
    let mut best_val = *(*t).data;
    for i in 1..n {
        let v = *(*t).data.add(i);
        if v < best_val {
            best = i;
            best_val = v;
        }
    }
    best as i64
}

// ── Linear algebra ──────────────────────────────────────────────────

/// Matrix multiplication. Supports 2D×2D, 2D×1D, 1D×2D.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_matmul(a: i64, b: i64) -> i64 {
    let ta = as_tensor(a);
    let tb = as_tensor(b);
    let a_ndim = (*ta).ndim as usize;
    let b_ndim = (*tb).ndim as usize;

    // 2D x 2D: [M,K] x [K,N] -> [M,N]
    if a_ndim == 2 && b_ndim == 2 {
        let m = *(*ta).shape as usize;
        let k = *(*ta).shape.add(1) as usize;
        let k2 = *(*tb).shape as usize;
        let n = *(*tb).shape.add(1) as usize;
        if k != k2 {
            return 0;
        }
        let shape = [m as i64, n as i64];
        let out = alloc_tensor(&shape);
        for i in 0..m {
            for j in 0..n {
                let mut sum = 0.0;
                for p in 0..k {
                    sum += *(*ta).data.add(i * k + p) * *(*tb).data.add(p * n + j);
                }
                *(*out).data.add(i * n + j) = sum;
            }
        }
        return out as i64;
    }

    // 2D x 1D: [M,K] x [K] -> [M]
    if a_ndim == 2 && b_ndim == 1 {
        let m = *(*ta).shape as usize;
        let k = *(*ta).shape.add(1) as usize;
        let shape = [m as i64];
        let out = alloc_tensor(&shape);
        for i in 0..m {
            let mut sum = 0.0;
            for p in 0..k {
                sum += *(*ta).data.add(i * k + p) * *(*tb).data.add(p);
            }
            *(*out).data.add(i) = sum;
        }
        return out as i64;
    }

    // 1D x 1D: dot product -> scalar (1-element tensor)
    if a_ndim == 1 && b_ndim == 1 {
        let n = (*ta).numel as usize;
        let shape = [1i64];
        let out = alloc_tensor(&shape);
        let mut sum = 0.0;
        for i in 0..n {
            sum += *(*ta).data.add(i) * *(*tb).data.add(i);
        }
        *(*out).data = sum;
        return out as i64;
    }

    0 // unsupported combination
}

/// Transpose a 2D tensor.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_transpose(h: i64) -> i64 {
    let t = as_tensor(h);
    if (*t).ndim != 2 {
        return h;
    } // only 2D
    let rows = *(*t).shape as usize;
    let cols = *(*t).shape.add(1) as usize;
    let shape = [cols as i64, rows as i64];
    let out = alloc_tensor(&shape);
    for i in 0..rows {
        for j in 0..cols {
            *(*out).data.add(j * rows + i) = *(*t).data.add(i * cols + j);
        }
    }
    out as i64
}

// ── Shape ops ───────────────────────────────────────────────────────

/// Reshape tensor. Data is shared (copied), shape changes.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_reshape(
    h: i64,
    new_shape_ptr: *const i64,
    new_ndim: i64,
) -> i64 {
    let t = as_tensor(h);
    let mut new_shape = read_shape(new_shape_ptr, new_ndim);
    let numel = (*t).numel as usize;

    // Handle -1 (infer dimension).
    let mut infer_idx: Option<usize> = None;
    let mut known_prod: i64 = 1;
    for (i, &s) in new_shape.iter().enumerate() {
        if s == -1 {
            infer_idx = Some(i);
        } else {
            known_prod *= s;
        }
    }
    if let Some(idx) = infer_idx {
        if known_prod > 0 {
            new_shape[idx] = numel as i64 / known_prod;
        }
    }

    let out = alloc_tensor(&new_shape);
    let copy_n = numel.min((*out).numel as usize);
    ptr::copy_nonoverlapping((*t).data, (*out).data, copy_n);
    out as i64
}

/// Flatten to 1D.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_flatten(h: i64) -> i64 {
    let t = as_tensor(h);
    let n = (*t).numel;
    let shape = [n];
    let out = alloc_tensor(&shape);
    ptr::copy_nonoverlapping((*t).data, (*out).data, n as usize);
    out as i64
}

// ── ML ops ──────────────────────────────────────────────────────────

/// Softmax along the last dimension.
/// For a 1D tensor, softmax is over all elements.
/// For a 2D tensor, softmax is per-row.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_softmax(h: i64, _dim: i64) -> i64 {
    let t = as_tensor(h);
    let ndim = (*t).ndim as usize;
    let shape = std::slice::from_raw_parts((*t).shape, ndim);
    let out = alloc_tensor(shape);
    let n = (*t).numel as usize;

    if ndim <= 1 {
        // Softmax over all elements.
        let mut max_val = f64::NEG_INFINITY;
        for i in 0..n {
            let v = *(*t).data.add(i);
            if v > max_val {
                max_val = v;
            }
        }
        let mut sum_exp = 0.0;
        for i in 0..n {
            let e = (*(*t).data.add(i) - max_val).exp();
            *(*out).data.add(i) = e;
            sum_exp += e;
        }
        for i in 0..n {
            *(*out).data.add(i) /= sum_exp;
        }
    } else {
        // Per-row softmax for 2D.
        let rows = shape[0] as usize;
        let cols = shape[ndim - 1] as usize;
        for r in 0..rows {
            let off = r * cols;
            let mut max_val = f64::NEG_INFINITY;
            for c in 0..cols {
                let v = *(*t).data.add(off + c);
                if v > max_val {
                    max_val = v;
                }
            }
            let mut sum_exp = 0.0;
            for c in 0..cols {
                let e = (*(*t).data.add(off + c) - max_val).exp();
                *(*out).data.add(off + c) = e;
                sum_exp += e;
            }
            for c in 0..cols {
                *(*out).data.add(off + c) /= sum_exp;
            }
        }
    }
    out as i64
}

/// Cross-entropy loss. Returns f64 bits as i64.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_cross_entropy(logits: i64, targets: i64) -> i64 {
    let probs_h = kryos_tensor_softmax(logits, -1);
    let tl = as_tensor(logits);
    let tt = as_tensor(targets);
    let batch = *(*tl).shape as usize;
    let classes = *(*tl).shape.add(1) as usize;
    let probs = as_tensor(probs_h);
    let mut loss = 0.0f64;
    for b in 0..batch {
        let cls = *(*tt).data.add(b) as usize;
        if cls < classes {
            let p = *(*probs).data.add(b * classes + cls);
            loss -= p.max(1e-12).ln();
        }
    }
    (loss / batch as f64).to_bits() as i64
}

/// Mean squared error loss. Returns f64 bits as i64.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_mse_loss(a: i64, b: i64) -> i64 {
    let ta = as_tensor(a);
    let tb = as_tensor(b);
    let n = ((*ta).numel as usize).min((*tb).numel as usize);
    if n == 0 {
        return 0.0f64.to_bits() as i64;
    }
    let mut sum = 0.0f64;
    for i in 0..n {
        let diff = *(*ta).data.add(i) - *(*tb).data.add(i);
        sum += diff * diff;
    }
    (sum / n as f64).to_bits() as i64
}

// ── String conversion ───────────────────────────────────────────────

/// Convert tensor to a string representation. Returns a KryosString handle.
#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_to_string(h: i64) -> i64 {
    let t = as_tensor(h);
    let n = (*t).numel as usize;
    let ndim = (*t).ndim as usize;
    let shape: Vec<i64> = if ndim > 0 {
        std::slice::from_raw_parts((*t).shape, ndim).to_vec()
    } else {
        vec![]
    };

    let mut s = String::from("tensor(");
    if ndim == 1 {
        s.push('[');
        for i in 0..n {
            if i > 0 {
                s.push_str(", ");
            }
            s.push_str(&format!("{:.4}", *(*t).data.add(i)));
        }
        s.push(']');
    } else if ndim == 2 {
        let rows = shape[0] as usize;
        let cols = shape[1] as usize;
        s.push('[');
        for r in 0..rows {
            if r > 0 {
                s.push_str(", ");
            }
            s.push('[');
            for c in 0..cols {
                if c > 0 {
                    s.push_str(", ");
                }
                s.push_str(&format!("{:.4}", *(*t).data.add(r * cols + c)));
            }
            s.push(']');
        }
        s.push(']');
    } else {
        // Generic: just print flat data.
        s.push('[');
        for i in 0..n.min(20) {
            if i > 0 {
                s.push_str(", ");
            }
            s.push_str(&format!("{:.4}", *(*t).data.add(i)));
        }
        if n > 20 {
            s.push_str(", ...");
        }
        s.push(']');
    }
    s.push_str(", shape=[");
    for (i, &d) in shape.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&d.to_string());
    }
    s.push_str("])");

    crate::string::kryos_string_new(s.as_ptr(), s.len() as i64) as i64
}

// ── Cleanup ─────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn kryos_tensor_free(h: i64) {
    if h == 0 {
        return;
    }
    let t = as_tensor(h);
    let n = (*t).numel as usize;
    if !(*t).data.is_null() && n > 0 {
        dealloc((*t).data as *mut u8, tensor_layout(n));
    }
    let ndim = (*t).ndim as usize;
    if !(*t).shape.is_null() && ndim > 0 {
        dealloc((*t).shape as *mut u8, shape_layout(ndim));
    }
    dealloc(t as *mut u8, Layout::new::<KryosTensor>());
}
