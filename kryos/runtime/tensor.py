"""
Kryos Tensor Primitives

N-dimensional tensor library with shape tracking, broadcasting, element-wise
operations, reductions, linear algebra, ML ops, and basic reverse-mode autodiff.

Uses numpy as the computation backend when available; falls back to pure-Python
nested lists so the module works anywhere.
"""

from __future__ import annotations

import math
import random
from typing import Any, Callable, List, Optional, Sequence, Tuple, Union

try:
    import numpy as np

    HAS_NUMPY = True
except ImportError:  # pragma: no cover
    np = None  # type: ignore[assignment]
    HAS_NUMPY = False

# ---------------------------------------------------------------------------
# Dtype helpers
# ---------------------------------------------------------------------------

_DTYPE_MAP_NP = {
    "f32": "float32",
    "f64": "float64",
    "i32": "int32",
    "i64": "int64",
    "bool": "bool",
}

_DTYPE_DEFAULT_VALUE = {
    "f32": 0.0,
    "f64": 0.0,
    "i32": 0,
    "i64": 0,
    "bool": False,
}


def _np_dtype(dtype: str):
    return _DTYPE_MAP_NP.get(dtype, dtype)


# ---------------------------------------------------------------------------
# Pure-Python helpers (used when numpy is not available)
# ---------------------------------------------------------------------------


def _flat_size(shape: Tuple[int, ...]) -> int:
    s = 1
    for d in shape:
        s *= d
    return s


def _flat_to_nested(flat: list, shape: Tuple[int, ...]) -> Any:
    """Convert a flat list to nested lists matching *shape*."""
    if len(shape) == 0:
        return flat[0]
    if len(shape) == 1:
        return list(flat)
    stride = _flat_size(shape[1:])
    return [_flat_to_nested(flat[i * stride : (i + 1) * stride], shape[1:]) for i in range(shape[0])]


def _nested_to_flat(data: Any) -> list:
    if isinstance(data, (list, tuple)):
        out: list = []
        for item in data:
            out.extend(_nested_to_flat(item))
        return out
    return [data]


def _infer_shape(data: Any) -> Tuple[int, ...]:
    if not isinstance(data, (list, tuple)):
        return ()
    if len(data) == 0:
        return (0,)
    inner = _infer_shape(data[0])
    return (len(data),) + inner


def _broadcast_shapes(a: Tuple[int, ...], b: Tuple[int, ...]) -> Tuple[int, ...]:
    """Compute the broadcast shape of *a* and *b* (numpy rules)."""
    ndim = max(len(a), len(b))
    a = (1,) * (ndim - len(a)) + a
    b = (1,) * (ndim - len(b)) + b
    result: list[int] = []
    for da, db in zip(a, b):
        if da == db:
            result.append(da)
        elif da == 1:
            result.append(db)
        elif db == 1:
            result.append(da)
        else:
            raise ValueError(f"shapes {a} and {b} are not broadcastable")
    return tuple(result)


def _broadcast_index(index: Tuple[int, ...], shape: Tuple[int, ...], target_shape: Tuple[int, ...]) -> Tuple[int, ...]:
    """Map an index in *target_shape* back to *shape* (handling broadcast dims)."""
    pad = len(target_shape) - len(shape)
    out: list[int] = []
    for i, s in enumerate(shape):
        ti = index[i + pad]
        out.append(0 if s == 1 else ti)
    return tuple(out)


def _flat_index(index: Tuple[int, ...], shape: Tuple[int, ...]) -> int:
    idx = 0
    stride = 1
    for i in range(len(shape) - 1, -1, -1):
        idx += index[i] * stride
        stride *= shape[i]
    return idx


def _get_element(flat: list, shape: Tuple[int, ...], index: Tuple[int, ...]) -> Any:
    return flat[_flat_index(index, shape)]


# ---------------------------------------------------------------------------
# KryosTensor
# ---------------------------------------------------------------------------


class KryosTensor:
    """
    N-dimensional tensor with shape tracking, broadcasting, and basic ML ops.
    Uses numpy if available, falls back to pure Python lists.
    """

    __slots__ = ("_data", "_shape", "_dtype")

    def __init__(self, data: Any, shape: Tuple[int, ...], dtype: str = "f32") -> None:
        self._data = data  # numpy ndarray or flat Python list
        self._shape = tuple(shape)
        self._dtype = dtype

    # -- properties ---------------------------------------------------------

    @property
    def data(self) -> Any:
        return self._data

    @property
    def shape(self) -> Tuple[int, ...]:
        return self._shape

    @property
    def dtype(self) -> str:
        return self._dtype

    @property
    def ndim(self) -> int:
        return len(self._shape)

    @property
    def numel(self) -> int:
        return _flat_size(self._shape)

    @property
    def T(self) -> KryosTensor:
        return self.transpose()

    # -- internal helpers ---------------------------------------------------

    def _is_np(self) -> bool:
        return HAS_NUMPY and isinstance(self._data, np.ndarray)

    def _to_np(self):
        if self._is_np():
            return self._data
        return np.array(self._flat(), dtype=_np_dtype(self._dtype)).reshape(self._shape)

    def _flat(self) -> list:
        """Return a flat Python list of elements."""
        if self._is_np():
            return self._data.ravel().tolist()
        return list(self._data)  # already flat internally in pure-Python path

    def _to_list(self) -> Any:
        """Return nested Python list mirroring shape."""
        return _flat_to_nested(self._flat(), self._shape)

    @staticmethod
    def _wrap_np(arr, dtype: str = "f32") -> KryosTensor:
        return KryosTensor(arr, tuple(arr.shape), dtype)

    @staticmethod
    def _wrap_flat(flat: list, shape: Tuple[int, ...], dtype: str = "f32") -> KryosTensor:
        return KryosTensor(flat, shape, dtype)

    # -- construction class methods -----------------------------------------

    @classmethod
    def zeros(cls, shape: Tuple[int, ...], dtype: str = "f32") -> KryosTensor:
        if HAS_NUMPY:
            return cls._wrap_np(np.zeros(shape, dtype=_np_dtype(dtype)), dtype)
        return cls._wrap_flat([_DTYPE_DEFAULT_VALUE.get(dtype, 0.0)] * _flat_size(shape), shape, dtype)

    @classmethod
    def ones(cls, shape: Tuple[int, ...], dtype: str = "f32") -> KryosTensor:
        if HAS_NUMPY:
            return cls._wrap_np(np.ones(shape, dtype=_np_dtype(dtype)), dtype)
        val = 1.0 if dtype.startswith("f") else 1
        return cls._wrap_flat([val] * _flat_size(shape), shape, dtype)

    @classmethod
    def rand(cls, shape: Tuple[int, ...], dtype: str = "f32") -> KryosTensor:
        if HAS_NUMPY:
            return cls._wrap_np(np.random.rand(*shape).astype(_np_dtype(dtype)), dtype)
        return cls._wrap_flat([random.random() for _ in range(_flat_size(shape))], shape, dtype)

    @classmethod
    def randn(cls, shape: Tuple[int, ...], dtype: str = "f32") -> KryosTensor:
        if HAS_NUMPY:
            return cls._wrap_np(np.random.randn(*shape).astype(_np_dtype(dtype)), dtype)
        return cls._wrap_flat([random.gauss(0, 1) for _ in range(_flat_size(shape))], shape, dtype)

    @classmethod
    def from_list(cls, data: Any, dtype: str = "f32") -> KryosTensor:
        shape = _infer_shape(data)
        if HAS_NUMPY:
            return cls._wrap_np(np.array(data, dtype=_np_dtype(dtype)), dtype)
        flat = _nested_to_flat(data)
        return cls._wrap_flat(flat, shape, dtype)

    @classmethod
    def arange(cls, start: float, end: float, step: float = 1.0, dtype: str = "f32") -> KryosTensor:
        if HAS_NUMPY:
            arr = np.arange(start, end, step, dtype=_np_dtype(dtype))
            return cls._wrap_np(arr, dtype)
        vals: list = []
        v = start
        while (step > 0 and v < end) or (step < 0 and v > end):
            vals.append(v)
            v += step
        return cls._wrap_flat(vals, (len(vals),), dtype)

    @classmethod
    def eye(cls, n: int, dtype: str = "f32") -> KryosTensor:
        if HAS_NUMPY:
            return cls._wrap_np(np.eye(n, dtype=_np_dtype(dtype)), dtype)
        flat = [0.0] * (n * n)
        for i in range(n):
            flat[i * n + i] = 1.0
        return cls._wrap_flat(flat, (n, n), dtype)

    @classmethod
    def full(cls, shape: Tuple[int, ...], value: float, dtype: str = "f32") -> KryosTensor:
        if HAS_NUMPY:
            return cls._wrap_np(np.full(shape, value, dtype=_np_dtype(dtype)), dtype)
        return cls._wrap_flat([value] * _flat_size(shape), shape, dtype)

    # -- element-wise ops ---------------------------------------------------

    def _elementwise_binary(self, other: Any, np_op: str, py_fn: Callable) -> KryosTensor:
        if not isinstance(other, KryosTensor):
            other = KryosTensor.full(self._shape, float(other), self._dtype)
        if self._is_np() and other._is_np():
            a = self._to_np()
            b = other._to_np()
            result = getattr(a, np_op)(b) if hasattr(a, np_op) else py_fn(a, b)
            if isinstance(result, np.ndarray):
                return KryosTensor._wrap_np(result, self._dtype)
        # Pure-python path with broadcasting
        out_shape = _broadcast_shapes(self._shape, other._shape)
        n = _flat_size(out_shape)
        a_flat = self._flat()
        b_flat = other._flat()
        result_flat: list = []
        for i in range(n):
            # Convert flat index to nd index in out_shape
            nd = []
            tmp = i
            for d in reversed(out_shape):
                nd.append(tmp % d)
                tmp //= d
            nd.reverse()
            nd_tuple = tuple(nd)
            ai = _flat_index(_broadcast_index(nd_tuple, self._shape, out_shape), self._shape)
            bi = _flat_index(_broadcast_index(nd_tuple, other._shape, out_shape), other._shape)
            result_flat.append(py_fn(a_flat[ai], b_flat[bi]))
        return KryosTensor._wrap_flat(result_flat, out_shape, self._dtype)

    def _elementwise_unary(self, np_fn: Callable | None, py_fn: Callable) -> KryosTensor:
        if self._is_np() and np_fn is not None:
            return KryosTensor._wrap_np(np_fn(self._to_np()), self._dtype)
        flat = self._flat()
        return KryosTensor._wrap_flat([py_fn(x) for x in flat], self._shape, self._dtype)

    def __add__(self, other: Any) -> KryosTensor:
        return self._elementwise_binary(other, "__add__", lambda a, b: a + b)

    def __radd__(self, other: Any) -> KryosTensor:
        return self.__add__(other)

    def __sub__(self, other: Any) -> KryosTensor:
        return self._elementwise_binary(other, "__sub__", lambda a, b: a - b)

    def __rsub__(self, other: Any) -> KryosTensor:
        if not isinstance(other, KryosTensor):
            other = KryosTensor.full(self._shape, float(other), self._dtype)
        return other.__sub__(self)

    def __mul__(self, other: Any) -> KryosTensor:
        return self._elementwise_binary(other, "__mul__", lambda a, b: a * b)

    def __rmul__(self, other: Any) -> KryosTensor:
        return self.__mul__(other)

    def __truediv__(self, other: Any) -> KryosTensor:
        return self._elementwise_binary(other, "__truediv__", lambda a, b: a / b)

    def __rtruediv__(self, other: Any) -> KryosTensor:
        if not isinstance(other, KryosTensor):
            other = KryosTensor.full(self._shape, float(other), self._dtype)
        return other.__truediv__(self)

    def __pow__(self, other: Any) -> KryosTensor:
        return self._elementwise_binary(other, "__pow__", lambda a, b: a ** b)

    def __neg__(self) -> KryosTensor:
        return self._elementwise_unary(lambda a: -a if HAS_NUMPY else None, lambda x: -x)

    def __abs__(self) -> KryosTensor:
        return self._elementwise_unary(np.abs if HAS_NUMPY else None, abs)

    def __eq__(self, other: Any) -> KryosTensor:  # type: ignore[override]
        return self._elementwise_binary(other, "__eq__", lambda a, b: float(a == b))

    def __lt__(self, other: Any) -> KryosTensor:
        return self._elementwise_binary(other, "__lt__", lambda a, b: float(a < b))

    def __gt__(self, other: Any) -> KryosTensor:
        return self._elementwise_binary(other, "__gt__", lambda a, b: float(a > b))

    # -- unary math ---------------------------------------------------------

    def exp(self) -> KryosTensor:
        return self._elementwise_unary(np.exp if HAS_NUMPY else None, math.exp)

    def log(self) -> KryosTensor:
        return self._elementwise_unary(np.log if HAS_NUMPY else None, math.log)

    def sqrt(self) -> KryosTensor:
        return self._elementwise_unary(np.sqrt if HAS_NUMPY else None, math.sqrt)

    def tanh(self) -> KryosTensor:
        return self._elementwise_unary(np.tanh if HAS_NUMPY else None, math.tanh)

    def sigmoid(self) -> KryosTensor:
        def _sig(x: float) -> float:
            if x >= 0:
                return 1.0 / (1.0 + math.exp(-x))
            ex = math.exp(x)
            return ex / (1.0 + ex)

        if self._is_np():
            a = self._to_np()
            return KryosTensor._wrap_np(1.0 / (1.0 + np.exp(-a)), self._dtype)
        return self._elementwise_unary(None, _sig)

    def relu(self) -> KryosTensor:
        return self._elementwise_unary(
            (lambda a: np.maximum(a, 0)) if HAS_NUMPY else None,
            lambda x: max(0.0, x),
        )

    # -- reductions ---------------------------------------------------------

    def _reduce(self, np_method: str, py_fn: Callable, axis: Optional[int] = None) -> KryosTensor:
        if self._is_np():
            arr = self._to_np()
            result = getattr(arr, np_method)(axis=axis)
            if isinstance(result, np.ndarray):
                return KryosTensor._wrap_np(result.astype(_np_dtype(self._dtype)), self._dtype)
            # scalar
            return KryosTensor._wrap_flat([float(result)], (), self._dtype)

        flat = self._flat()
        if axis is None:
            val = py_fn(flat)
            return KryosTensor._wrap_flat([val], (), self._dtype)

        # axis-aware reduction (pure python)
        if axis < 0:
            axis = len(self._shape) + axis
        out_shape = self._shape[:axis] + self._shape[axis + 1 :]
        out_size = _flat_size(out_shape) if out_shape else 1
        # gather slices along axis
        result_flat: list = []
        for oi in range(out_size):
            # decode oi -> nd index in out_shape
            nd = []
            tmp = oi
            for d in reversed(out_shape):
                nd.append(tmp % d)
                tmp //= d
            nd.reverse()
            # collect elements along the reduction axis
            group: list = []
            for k in range(self._shape[axis]):
                full_idx = list(nd)
                full_idx.insert(axis, k)
                group.append(flat[_flat_index(tuple(full_idx), self._shape)])
            result_flat.append(py_fn(group))
        return KryosTensor._wrap_flat(result_flat, out_shape if out_shape else (), self._dtype)

    def sum(self, axis: Optional[int] = None) -> KryosTensor:
        return self._reduce("sum", sum, axis)

    def mean(self, axis: Optional[int] = None) -> KryosTensor:
        return self._reduce("mean", lambda xs: sum(xs) / len(xs), axis)

    def max(self, axis: Optional[int] = None) -> KryosTensor:
        return self._reduce("max", max, axis)

    def min(self, axis: Optional[int] = None) -> KryosTensor:
        return self._reduce("min", min, axis)

    def argmax(self, axis: Optional[int] = None) -> KryosTensor:
        def _argmax(xs: list) -> int:
            return xs.index(max(xs))

        if self._is_np():
            result = self._to_np().argmax(axis=axis)
            if isinstance(result, np.ndarray):
                return KryosTensor._wrap_np(result.astype("int64"), "i64")
            return KryosTensor._wrap_flat([int(result)], (), "i64")
        return self._reduce("argmax", _argmax, axis)

    def argmin(self, axis: Optional[int] = None) -> KryosTensor:
        def _argmin(xs: list) -> int:
            return xs.index(min(xs))

        if self._is_np():
            result = self._to_np().argmin(axis=axis)
            if isinstance(result, np.ndarray):
                return KryosTensor._wrap_np(result.astype("int64"), "i64")
            return KryosTensor._wrap_flat([int(result)], (), "i64")
        return self._reduce("argmin", _argmin, axis)

    # -- linear algebra -----------------------------------------------------

    def matmul(self, other: KryosTensor) -> KryosTensor:
        """Matrix multiplication (supports 2-D tensors)."""
        if self.ndim < 1 or other.ndim < 1:
            raise ValueError("matmul requires at least 1-D tensors")
        if self._is_np() and other._is_np():
            result = self._to_np() @ other._to_np()
            return KryosTensor._wrap_np(result.astype(_np_dtype(self._dtype)), self._dtype)

        # Pure-python: handles 2D x 2D, 2D x 1D, 1D x 2D
        a_flat = self._flat()
        b_flat = other._flat()

        if self.ndim == 2 and other.ndim == 2:
            m, k1 = self._shape
            k2, n = other._shape
            if k1 != k2:
                raise ValueError(f"matmul shape mismatch: {self._shape} @ {other._shape}")
            out = [0.0] * (m * n)
            for i in range(m):
                for j in range(n):
                    s = 0.0
                    for p in range(k1):
                        s += a_flat[i * k1 + p] * b_flat[p * n + j]
                    out[i * n + j] = s
            return KryosTensor._wrap_flat(out, (m, n), self._dtype)

        if self.ndim == 2 and other.ndim == 1:
            m, k = self._shape
            if k != other._shape[0]:
                raise ValueError(f"matmul shape mismatch: {self._shape} @ {other._shape}")
            out = [0.0] * m
            for i in range(m):
                s = 0.0
                for p in range(k):
                    s += a_flat[i * k + p] * b_flat[p]
                out[i] = s
            return KryosTensor._wrap_flat(out, (m,), self._dtype)

        if self.ndim == 1 and other.ndim == 2:
            k, n = other._shape
            if self._shape[0] != k:
                raise ValueError(f"matmul shape mismatch: {self._shape} @ {other._shape}")
            out = [0.0] * n
            for j in range(n):
                s = 0.0
                for p in range(k):
                    s += a_flat[p] * b_flat[p * n + j]
                out[j] = s
            return KryosTensor._wrap_flat(out, (n,), self._dtype)

        if self.ndim == 1 and other.ndim == 1:
            return self.dot(other)

        # Batched matmul for higher dimensions
        if self._is_np() and other._is_np():
            result = np.matmul(self._to_np(), other._to_np())
            return KryosTensor._wrap_np(result, self._dtype)

        raise ValueError(f"matmul not implemented for {self.ndim}-D and {other.ndim}-D in pure-Python mode")

    def __matmul__(self, other: KryosTensor) -> KryosTensor:
        return self.matmul(other)

    def dot(self, other: KryosTensor) -> KryosTensor:
        """Vector dot product."""
        if self.ndim != 1 or other.ndim != 1:
            raise ValueError("dot requires 1-D tensors")
        if self._shape[0] != other._shape[0]:
            raise ValueError("dot requires tensors of same length")
        if self._is_np() and other._is_np():
            val = float(np.dot(self._to_np(), other._to_np()))
            return KryosTensor._wrap_flat([val], (), self._dtype)
        a = self._flat()
        b = other._flat()
        return KryosTensor._wrap_flat([sum(x * y for x, y in zip(a, b))], (), self._dtype)

    def transpose(self, *axes: int) -> KryosTensor:
        """Transpose (default: reverse all axes)."""
        if self._is_np():
            if axes:
                return KryosTensor._wrap_np(self._to_np().transpose(*axes), self._dtype)
            return KryosTensor._wrap_np(self._to_np().T, self._dtype)
        if self.ndim < 2:
            return KryosTensor._wrap_flat(list(self._flat()), self._shape, self._dtype)
        if self.ndim == 2:
            m, n = self._shape
            flat = self._flat()
            out = [0.0] * (m * n)
            for i in range(m):
                for j in range(n):
                    out[j * m + i] = flat[i * n + j]
            return KryosTensor._wrap_flat(out, (n, m), self._dtype)
        # General transpose
        if not axes:
            axes_list = list(range(self.ndim - 1, -1, -1))
        else:
            axes_list = list(axes)
        new_shape = tuple(self._shape[a] for a in axes_list)
        n = self.numel
        flat = self._flat()
        out = [0.0] * n
        for i in range(n):
            # decode i in new_shape
            nd_new: list[int] = []
            tmp = i
            for d in reversed(new_shape):
                nd_new.append(tmp % d)
                tmp //= d
            nd_new.reverse()
            # map back to old shape
            nd_old = [0] * self.ndim
            for a_idx, orig_axis in enumerate(axes_list):
                nd_old[orig_axis] = nd_new[a_idx]
            old_fi = _flat_index(tuple(nd_old), self._shape)
            out[i] = flat[old_fi]
        return KryosTensor._wrap_flat(out, new_shape, self._dtype)

    def reshape(self, new_shape: Tuple[int, ...]) -> KryosTensor:
        new_shape = tuple(new_shape)
        # Handle -1 in shape
        if -1 in new_shape:
            known = 1
            neg_idx = -1
            for i, d in enumerate(new_shape):
                if d == -1:
                    if neg_idx >= 0:
                        raise ValueError("only one -1 allowed in reshape")
                    neg_idx = i
                else:
                    known *= d
            inferred = self.numel // known
            new_shape = new_shape[:neg_idx] + (inferred,) + new_shape[neg_idx + 1 :]
        if _flat_size(new_shape) != self.numel:
            raise ValueError(f"cannot reshape {self._shape} to {new_shape}")
        if self._is_np():
            return KryosTensor._wrap_np(self._to_np().reshape(new_shape), self._dtype)
        return KryosTensor._wrap_flat(list(self._flat()), new_shape, self._dtype)

    def view(self, shape: Tuple[int, ...]) -> KryosTensor:
        """Reshape alias."""
        return self.reshape(shape)

    def flatten(self) -> KryosTensor:
        return self.reshape((self.numel,))

    # -- shape operations ---------------------------------------------------

    def squeeze(self, dim: Optional[int] = None) -> KryosTensor:
        if dim is not None:
            if dim < 0:
                dim = self.ndim + dim
            if self._shape[dim] != 1:
                return KryosTensor._wrap_flat(list(self._flat()), self._shape, self._dtype)
            new_shape = self._shape[:dim] + self._shape[dim + 1 :]
        else:
            new_shape = tuple(d for d in self._shape if d != 1)
        if not new_shape:
            new_shape = ()
        if self._is_np():
            return KryosTensor._wrap_np(self._to_np().reshape(new_shape if new_shape else (1,)), self._dtype)
        return KryosTensor._wrap_flat(list(self._flat()), new_shape if new_shape else (), self._dtype)

    def unsqueeze(self, dim: int) -> KryosTensor:
        if dim < 0:
            dim = self.ndim + 1 + dim
        new_shape = self._shape[:dim] + (1,) + self._shape[dim:]
        if self._is_np():
            return KryosTensor._wrap_np(self._to_np().reshape(new_shape), self._dtype)
        return KryosTensor._wrap_flat(list(self._flat()), new_shape, self._dtype)

    @staticmethod
    def cat(tensors: List[KryosTensor], dim: int = 0) -> KryosTensor:
        if not tensors:
            raise ValueError("cat requires at least one tensor")
        if dim < 0:
            dim = tensors[0].ndim + dim
        if HAS_NUMPY and all(t._is_np() for t in tensors):
            result = np.concatenate([t._to_np() for t in tensors], axis=dim)
            return KryosTensor._wrap_np(result, tensors[0]._dtype)
        # Pure-python concat
        base_shape = list(tensors[0]._shape)
        cat_size = 0
        for t in tensors:
            for i, (a, b) in enumerate(zip(base_shape, t._shape)):
                if i != dim and a != b:
                    raise ValueError("tensor shapes do not match for concatenation")
            cat_size += t._shape[dim]
        out_shape = list(base_shape)
        out_shape[dim] = cat_size
        out_shape_t = tuple(out_shape)

        # Build output by iterating through output indices
        out_size = _flat_size(out_shape_t)
        result_flat: list = []
        for i in range(out_size):
            nd: list[int] = []
            tmp = i
            for d in reversed(out_shape_t):
                nd.append(tmp % d)
                tmp //= d
            nd.reverse()
            # Figure out which tensor and local index along dim
            dim_idx = nd[dim]
            offset = 0
            for t in tensors:
                if dim_idx < offset + t._shape[dim]:
                    local_nd = list(nd)
                    local_nd[dim] = dim_idx - offset
                    val = t._flat()[_flat_index(tuple(local_nd), t._shape)]
                    result_flat.append(val)
                    break
                offset += t._shape[dim]
        return KryosTensor._wrap_flat(result_flat, out_shape_t, tensors[0]._dtype)

    @staticmethod
    def stack(tensors: List[KryosTensor], dim: int = 0) -> KryosTensor:
        if not tensors:
            raise ValueError("stack requires at least one tensor")
        expanded = [t.unsqueeze(dim) for t in tensors]
        return KryosTensor.cat(expanded, dim)

    def split(self, sections: int, dim: int = 0) -> List[KryosTensor]:
        if dim < 0:
            dim = self.ndim + dim
        size = self._shape[dim]
        chunk_size = size // sections
        remainder = size % sections
        chunks: list[KryosTensor] = []
        start = 0
        for i in range(sections):
            cs = chunk_size + (1 if i < remainder else 0)
            chunks.append(self.slice(dim, start, start + cs))
            start += cs
        return chunks

    def slice(self, dim: int, start: int, end: int) -> KryosTensor:
        if dim < 0:
            dim = self.ndim + dim
        if self._is_np():
            slices = [builtins_slice(None)] * self.ndim
            slices[dim] = builtins_slice(start, end)
            result = self._to_np()[tuple(slices)]
            return KryosTensor._wrap_np(result.copy(), self._dtype)
        # Pure-python
        new_shape = list(self._shape)
        new_shape[dim] = end - start
        new_shape_t = tuple(new_shape)
        out_size = _flat_size(new_shape_t)
        flat = self._flat()
        result_flat: list = []
        for i in range(out_size):
            nd: list[int] = []
            tmp = i
            for d in reversed(new_shape_t):
                nd.append(tmp % d)
                tmp //= d
            nd.reverse()
            src_nd = list(nd)
            src_nd[dim] += start
            result_flat.append(flat[_flat_index(tuple(src_nd), self._shape)])
        return KryosTensor._wrap_flat(result_flat, new_shape_t, self._dtype)

    # -- ML-specific operations ---------------------------------------------

    def softmax(self, axis: int = -1) -> KryosTensor:
        if axis < 0:
            axis = self.ndim + axis
        if self._is_np():
            a = self._to_np()
            e = np.exp(a - np.max(a, axis=axis, keepdims=True))
            return KryosTensor._wrap_np((e / np.sum(e, axis=axis, keepdims=True)).astype(_np_dtype(self._dtype)), self._dtype)
        # Pure-python softmax along axis
        flat = self._flat()
        out = list(flat)
        # Iterate over all positions except the softmax axis
        outer_shape = self._shape[:axis] + self._shape[axis + 1 :]
        outer_size = _flat_size(outer_shape) if outer_shape else 1
        for oi in range(outer_size):
            nd: list[int] = []
            tmp = oi
            for d in reversed(outer_shape):
                nd.append(tmp % d)
                tmp //= d
            nd.reverse()
            # Gather elements along axis
            indices: list[int] = []
            vals: list[float] = []
            for k in range(self._shape[axis]):
                full_nd = list(nd)
                full_nd.insert(axis, k)
                fi = _flat_index(tuple(full_nd), self._shape)
                indices.append(fi)
                vals.append(flat[fi])
            # Stable softmax
            mx = max(vals)
            exps = [math.exp(v - mx) for v in vals]
            s = sum(exps)
            for idx, e in zip(indices, exps):
                out[idx] = e / s
        return KryosTensor._wrap_flat(out, self._shape, self._dtype)

    def layer_norm(self, eps: float = 1e-5) -> KryosTensor:
        """Layer normalization over the last dimension."""
        if self._is_np():
            a = self._to_np()
            mean = np.mean(a, axis=-1, keepdims=True)
            var = np.var(a, axis=-1, keepdims=True)
            result = (a - mean) / np.sqrt(var + eps)
            return KryosTensor._wrap_np(result.astype(_np_dtype(self._dtype)), self._dtype)
        # Pure-python
        flat = self._flat()
        out = list(flat)
        last_dim = self._shape[-1]
        num_vecs = self.numel // last_dim
        for v in range(num_vecs):
            start = v * last_dim
            vals = flat[start : start + last_dim]
            m = sum(vals) / last_dim
            var = sum((x - m) ** 2 for x in vals) / last_dim
            inv_std = 1.0 / math.sqrt(var + eps)
            for i in range(last_dim):
                out[start + i] = (vals[i] - m) * inv_std
        return KryosTensor._wrap_flat(out, self._shape, self._dtype)

    def cross_entropy(self, target: KryosTensor) -> KryosTensor:
        """Cross-entropy loss. self = logits (N, C), target = class indices (N,)."""
        probs = self.softmax(axis=-1)
        if self._is_np() and probs._is_np():
            p = probs._to_np()
            t = target._to_np().astype(int)
            n = p.shape[0]
            log_probs = np.log(np.clip(p[np.arange(n), t], 1e-12, 1.0))
            loss = -np.mean(log_probs)
            return KryosTensor._wrap_flat([float(loss)], (), self._dtype)
        p_flat = probs._flat()
        t_flat = target._flat()
        n = self._shape[0]
        c = self._shape[-1]
        total = 0.0
        for i in range(n):
            cls_idx = int(t_flat[i])
            prob = p_flat[i * c + cls_idx]
            total -= math.log(max(prob, 1e-12))
        return KryosTensor._wrap_flat([total / n], (), self._dtype)

    def mse_loss(self, target: KryosTensor) -> KryosTensor:
        """Mean squared error loss."""
        diff = self - target
        sq = diff * diff
        return sq.mean()

    def dropout(self, p: float = 0.5, training: bool = True) -> KryosTensor:
        """Dropout: randomly zero out elements with probability p."""
        if not training or p == 0.0:
            return KryosTensor._wrap_flat(list(self._flat()), self._shape, self._dtype)
        scale = 1.0 / (1.0 - p)
        if self._is_np():
            mask = (np.random.rand(*self._shape) >= p).astype(_np_dtype(self._dtype))
            return KryosTensor._wrap_np((self._to_np() * mask * scale).astype(_np_dtype(self._dtype)), self._dtype)
        flat = self._flat()
        out = [(x * scale if random.random() >= p else 0.0) for x in flat]
        return KryosTensor._wrap_flat(out, self._shape, self._dtype)

    # -- repr ---------------------------------------------------------------

    def __repr__(self) -> str:
        if self.numel <= 20:
            data_str = str(self._to_list())
        else:
            flat = self._flat()
            data_str = str(flat[:5])[:-1] + f", ... ({self.numel} elements)]"
        return f"KryosTensor(shape={self._shape}, dtype={self._dtype}, data={data_str})"

    def item(self) -> float:
        """Return scalar value (only valid for 0-d or single-element tensors)."""
        if self.numel != 1:
            raise ValueError(f"item() requires single-element tensor, got {self.numel}")
        return self._flat()[0]


# Avoid name collision with Python's slice builtin used in .slice() method
builtins_slice = slice


# ---------------------------------------------------------------------------
# GradTensor — basic reverse-mode autodiff
# ---------------------------------------------------------------------------


class GradTensor:
    """
    Tensor that tracks gradients for backpropagation.
    Wraps a KryosTensor and records operations in a computational graph.
    """

    __slots__ = ("tensor", "requires_grad", "grad", "_backward_fn", "_children")

    def __init__(
        self,
        tensor: KryosTensor,
        requires_grad: bool = True,
        _children: Tuple[GradTensor, ...] = (),
        _backward_fn: Optional[Callable] = None,
    ) -> None:
        self.tensor = tensor
        self.requires_grad = requires_grad
        self.grad: Optional[KryosTensor] = None
        self._backward_fn = _backward_fn
        self._children = _children

    @property
    def shape(self) -> Tuple[int, ...]:
        return self.tensor.shape

    @property
    def dtype(self) -> str:
        return self.tensor.dtype

    @property
    def data(self) -> KryosTensor:
        return self.tensor

    @classmethod
    def wrap(cls, t: KryosTensor, requires_grad: bool = True) -> GradTensor:
        return cls(t, requires_grad=requires_grad)

    @classmethod
    def zeros(cls, shape: Tuple[int, ...], **kw: Any) -> GradTensor:
        return cls(KryosTensor.zeros(shape, **kw))

    @classmethod
    def ones(cls, shape: Tuple[int, ...], **kw: Any) -> GradTensor:
        return cls(KryosTensor.ones(shape, **kw))

    @classmethod
    def rand(cls, shape: Tuple[int, ...], **kw: Any) -> GradTensor:
        return cls(KryosTensor.rand(shape, **kw))

    @classmethod
    def randn(cls, shape: Tuple[int, ...], **kw: Any) -> GradTensor:
        return cls(KryosTensor.randn(shape, **kw))

    @classmethod
    def from_list(cls, data: Any, requires_grad: bool = True, **kw: Any) -> GradTensor:
        t = cls(KryosTensor.from_list(data, **kw))
        t.requires_grad = requires_grad
        return t

    # -- backward -----------------------------------------------------------

    def backward(self) -> None:
        """Compute gradients via reverse-mode autodiff (topological sort)."""
        if not self.requires_grad:
            return

        # Build topological order
        topo: list[GradTensor] = []
        visited: set[int] = set()

        def _build(node: GradTensor) -> None:
            nid = id(node)
            if nid in visited:
                return
            visited.add(nid)
            for child in node._children:
                _build(child)
            topo.append(node)

        _build(self)

        # Seed gradient
        self.grad = KryosTensor.ones(self.tensor.shape, dtype=self.tensor.dtype)

        # Reverse pass
        for node in reversed(topo):
            if node._backward_fn is not None:
                node._backward_fn()

    def zero_grad(self) -> None:
        self.grad = None

    # -- _ensure_grad helper ------------------------------------------------

    def _acc_grad(self, grad: KryosTensor) -> None:
        """Accumulate gradient."""
        if self.grad is None:
            self.grad = grad
        else:
            self.grad = self.grad + grad

    # -- ops with gradient tracking -----------------------------------------

    def __add__(self, other: Any) -> GradTensor:
        other = other if isinstance(other, GradTensor) else GradTensor(KryosTensor.full(self.tensor.shape, float(other), self.tensor.dtype), requires_grad=False)
        out_tensor = self.tensor + other.tensor
        out = GradTensor(out_tensor, requires_grad=True, _children=(self, other))

        def _backward() -> None:
            if out.grad is None:
                return
            g = out.grad
            if self.requires_grad:
                # Sum out broadcast dims if shapes differ
                self._acc_grad(_unbroadcast(g, self.tensor.shape))
            if other.requires_grad:
                other._acc_grad(_unbroadcast(g, other.tensor.shape))

        out._backward_fn = _backward
        return out

    def __radd__(self, other: Any) -> GradTensor:
        return self.__add__(other)

    def __mul__(self, other: Any) -> GradTensor:
        other = other if isinstance(other, GradTensor) else GradTensor(KryosTensor.full(self.tensor.shape, float(other), self.tensor.dtype), requires_grad=False)
        out_tensor = self.tensor * other.tensor
        out = GradTensor(out_tensor, requires_grad=True, _children=(self, other))

        def _backward() -> None:
            if out.grad is None:
                return
            g = out.grad
            if self.requires_grad:
                self._acc_grad(_unbroadcast(g * other.tensor, self.tensor.shape))
            if other.requires_grad:
                other._acc_grad(_unbroadcast(g * self.tensor, other.tensor.shape))

        out._backward_fn = _backward
        return out

    def __rmul__(self, other: Any) -> GradTensor:
        return self.__mul__(other)

    def __sub__(self, other: Any) -> GradTensor:
        other = other if isinstance(other, GradTensor) else GradTensor(KryosTensor.full(self.tensor.shape, float(other), self.tensor.dtype), requires_grad=False)
        out_tensor = self.tensor - other.tensor
        out = GradTensor(out_tensor, requires_grad=True, _children=(self, other))

        def _backward() -> None:
            if out.grad is None:
                return
            g = out.grad
            if self.requires_grad:
                self._acc_grad(_unbroadcast(g, self.tensor.shape))
            if other.requires_grad:
                other._acc_grad(_unbroadcast(-g.tensor if isinstance(g, GradTensor) else KryosTensor._wrap_flat([-x for x in g._flat()], g.shape, g.dtype), other.tensor.shape))

        out._backward_fn = _backward
        return out

    def __neg__(self) -> GradTensor:
        out_tensor = -self.tensor
        out = GradTensor(out_tensor, requires_grad=True, _children=(self,))

        def _backward() -> None:
            if out.grad is None:
                return
            if self.requires_grad:
                neg_grad = KryosTensor._wrap_flat([-x for x in out.grad._flat()], out.grad.shape, out.grad.dtype)
                self._acc_grad(neg_grad)

        out._backward_fn = _backward
        return out

    def __truediv__(self, other: Any) -> GradTensor:
        other = other if isinstance(other, GradTensor) else GradTensor(KryosTensor.full(self.tensor.shape, float(other), self.tensor.dtype), requires_grad=False)
        out_tensor = self.tensor / other.tensor
        out = GradTensor(out_tensor, requires_grad=True, _children=(self, other))

        def _backward() -> None:
            if out.grad is None:
                return
            g = out.grad
            if self.requires_grad:
                # d/da (a/b) = 1/b
                self._acc_grad(_unbroadcast(g / other.tensor, self.tensor.shape))
            if other.requires_grad:
                # d/db (a/b) = -a/b^2
                neg_a = KryosTensor._wrap_flat([-x for x in self.tensor._flat()], self.tensor.shape, self.tensor.dtype)
                b_sq = other.tensor * other.tensor
                other._acc_grad(_unbroadcast(g * neg_a / b_sq, other.tensor.shape))

        out._backward_fn = _backward
        return out

    def matmul(self, other: GradTensor) -> GradTensor:
        out_tensor = self.tensor.matmul(other.tensor)
        out = GradTensor(out_tensor, requires_grad=True, _children=(self, other))

        def _backward() -> None:
            if out.grad is None:
                return
            g = out.grad
            if self.requires_grad:
                # dL/dA = dL/dC @ B^T
                self._acc_grad(g.matmul(other.tensor.T))
            if other.requires_grad:
                # dL/dB = A^T @ dL/dC
                other._acc_grad(self.tensor.T.matmul(g))

        out._backward_fn = _backward
        return out

    def __matmul__(self, other: GradTensor) -> GradTensor:
        return self.matmul(other)

    def relu(self) -> GradTensor:
        out_tensor = self.tensor.relu()
        out = GradTensor(out_tensor, requires_grad=True, _children=(self,))

        def _backward() -> None:
            if out.grad is None:
                return
            if self.requires_grad:
                # relu grad: 1 where input > 0, else 0
                mask_flat = [1.0 if x > 0 else 0.0 for x in self.tensor._flat()]
                mask = KryosTensor._wrap_flat(mask_flat, self.tensor.shape, self.tensor.dtype)
                self._acc_grad(out.grad * mask)

        out._backward_fn = _backward
        return out

    def sum(self, axis: Optional[int] = None) -> GradTensor:
        out_tensor = self.tensor.sum(axis)
        out = GradTensor(out_tensor, requires_grad=True, _children=(self,))

        def _backward() -> None:
            if out.grad is None:
                return
            if self.requires_grad:
                # Gradient of sum is ones broadcast to input shape
                if axis is None:
                    g_val = out.grad.item()
                    self._acc_grad(KryosTensor.full(self.tensor.shape, g_val, self.tensor.dtype))
                else:
                    # Expand grad back along the reduced axis
                    g = out.grad
                    expanded = g.unsqueeze(axis)
                    # Broadcast to input shape
                    ones = KryosTensor.ones(self.tensor.shape, dtype=self.tensor.dtype)
                    self._acc_grad(ones * expanded)

        out._backward_fn = _backward
        return out

    def mean(self, axis: Optional[int] = None) -> GradTensor:
        out_tensor = self.tensor.mean(axis)
        out = GradTensor(out_tensor, requires_grad=True, _children=(self,))

        def _backward() -> None:
            if out.grad is None:
                return
            if self.requires_grad:
                if axis is None:
                    n = self.tensor.numel
                    g_val = out.grad.item() / n
                    self._acc_grad(KryosTensor.full(self.tensor.shape, g_val, self.tensor.dtype))
                else:
                    n = self.tensor.shape[axis]
                    g = out.grad
                    expanded = g.unsqueeze(axis)
                    ones = KryosTensor.ones(self.tensor.shape, dtype=self.tensor.dtype)
                    scale = KryosTensor.full(self.tensor.shape, 1.0 / n, self.tensor.dtype)
                    self._acc_grad(ones * expanded * scale)

        out._backward_fn = _backward
        return out

    def softmax(self, axis: int = -1) -> GradTensor:
        out_tensor = self.tensor.softmax(axis)
        out = GradTensor(out_tensor, requires_grad=True, _children=(self,))

        def _backward() -> None:
            if out.grad is None:
                return
            if self.requires_grad:
                # Softmax Jacobian: s_i * (delta_ij - s_j)
                # Simplified: grad_input = s * (grad_output - sum(grad_output * s, axis) )
                s = out.tensor
                g = out.grad
                sg = s * g
                sg_sum = sg.sum(axis=axis if axis >= 0 else axis)
                if sg_sum.ndim < s.ndim:
                    sg_sum = sg_sum.unsqueeze(axis if axis >= 0 else s.ndim + axis)
                ones = KryosTensor.ones(s.shape, dtype=s.dtype)
                self._acc_grad(s * (g - ones * sg_sum))

        out._backward_fn = _backward
        return out

    def __repr__(self) -> str:
        return f"GradTensor(grad={self.grad is not None}, {self.tensor!r})"


# ---------------------------------------------------------------------------
# Autodiff helpers
# ---------------------------------------------------------------------------


def _unbroadcast(grad: KryosTensor, target_shape: Tuple[int, ...]) -> KryosTensor:
    """Sum out broadcast dimensions so grad matches target_shape."""
    if grad.shape == target_shape:
        return grad
    if target_shape == ():
        return grad.sum()
    # Pad target_shape on the left
    ndim_diff = len(grad.shape) - len(target_shape)
    padded = (1,) * ndim_diff + target_shape
    result = grad
    # Sum over leading broadcast dims
    for _ in range(ndim_diff):
        result = result.sum(axis=0)
    # Sum over dims that were broadcast (size 1 in target)
    for i in range(len(target_shape)):
        if target_shape[i] == 1 and result.shape[i] != 1:
            result = result.sum(axis=i)
            result = result.unsqueeze(i)
    return result


# ---------------------------------------------------------------------------
# Module-level convenience (for Kryos interpreter builtins)
# ---------------------------------------------------------------------------


def tensor_from_list(data: Any) -> KryosTensor:
    return KryosTensor.from_list(data)


def tensor_zeros(shape: Any) -> KryosTensor:
    return KryosTensor.zeros(tuple(shape))


def tensor_ones(shape: Any) -> KryosTensor:
    return KryosTensor.ones(tuple(shape))


def tensor_rand(shape: Any) -> KryosTensor:
    return KryosTensor.rand(tuple(shape))


def tensor_randn(shape: Any) -> KryosTensor:
    return KryosTensor.randn(tuple(shape))


def tensor_eye(n: int) -> KryosTensor:
    return KryosTensor.eye(n)


def tensor_arange(start: float, end: float, step: float = 1.0) -> KryosTensor:
    return KryosTensor.arange(start, end, step)


def tensor_matmul(a: KryosTensor, b: KryosTensor) -> KryosTensor:
    return a.matmul(b)


def tensor_softmax(t: KryosTensor, axis: int = -1) -> KryosTensor:
    return t.softmax(axis)


def tensor_relu(t: KryosTensor) -> KryosTensor:
    return t.relu()


def tensor_reshape(t: KryosTensor, shape: Any) -> KryosTensor:
    return t.reshape(tuple(shape))


def tensor_cat(tensors: list, dim: int = 0) -> KryosTensor:
    return KryosTensor.cat(tensors, dim)


def tensor_sum(t: KryosTensor, axis: Any = None) -> KryosTensor:
    return t.sum(axis)


def tensor_mean(t: KryosTensor, axis: Any = None) -> KryosTensor:
    return t.mean(axis)
