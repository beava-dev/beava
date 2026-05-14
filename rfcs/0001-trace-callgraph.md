# Trace — function-call walkthrough, Python entry point → JSON bytes

Companion to [`0001-trace-current.md`](./0001-trace-current.md). Where
that file is structured around the *components* (DSL, decorators,
chain methods, app, server), this file walks through the **actual
function calls in execution order** for one script, from
`python script.py` to the bytes the transport hands to the socket.

Useful for building the mental model of "when does serialization
actually happen?" — the answer is *eagerly*, three lines, in two
files. Everything else is dict bookkeeping.

---

## The script being traced

```python
import beava as bv

@bv.event
class Purchase:
    user_id: str
    amount: float
    item:    str | None
    ts:      int

@bv.event
def UserStats(e: Purchase):
    e = e.with_columns(is_big = bv.col("amount") > 100.0)
    return e.group_by("user_id").agg(
        big_count_1h = bv.count(where=bv.col("is_big"),       window="1h"),
        total_24h    = bv.sum("amount",                       window="24h"),
        named_24h    = bv.count(where=bv.col("item").isnull() == False,
                                                              window="24h"),
    )

app = bv.App(...)
app.register(Purchase, UserStats)
```

---

## Phase A — Module load

**Step 1.** Python interpreter executes the script top-down.
`import beava as bv` runs `python/beava/__init__.py`, which
re-exports `event`, `col`, `lit`, `count`, `sum`, …, `App`. **No
registration happens here.**

---

## Phase B — `@bv.event class Purchase` decoration

**Step 2.** Python first builds the bare `Purchase` class object
(annotations live in `__annotations__`).

**Step 3.** Python calls `Purchase = event(Purchase)`. This invokes
`event(cls_or_fn=Purchase)` at `_events.py:359`.

**Step 4.** Inside `event` (`_events.py:387`):

```python
if cls_or_fn is None: ...                    # skip (factory branch)
if inspect.isclass(cls_or_fn):               # True
    return _make_event_source(cls_or_fn, {})
```

**Step 5.** `_make_event_source(cls=Purchase, kwargs={})` at
`_events.py:208`:

- `_validate_class_event(cls, {})` (`_events.py:188`) — rejects
  `event_time` field and `tolerate_delay` kwarg. Passes.
- `hints = get_type_hints(Purchase, include_extras=True)` — resolves
  `str | None` to `typing.Optional[str]`. Returns
  `{user_id: str, amount: float, item: Optional[str], ts: int}`.
- `src = EventSource(name="Purchase", schema=hints)` — runs
  `EventSource.__init__` at `_events.py:112`. Sets:

  ```
  src._name           = "Purchase"
  src._schema         = {...hints...}
  src._chain          = []           ← the list chain methods will append to
  src._kind           = "event_source"
  src._keep_events_for / _cold_after / _dedupe_key / _dedupe_window = None
  ```

- **Loop 1** (`_events.py:213-225`): `setattr(Purchase, attr,
  getattr(src, attr))` for each of `_name`, `_schema`, `_chain`,
  `_keep_events_for`, `_cold_after`, `_dedupe_key`,
  `_dedupe_window`, `_kind`. The class object now mirrors the
  EventSource state.
- **Loop 2** (`_events.py:226-241`): for each chain method
  (`filter`, `select`, `with_columns`, `group_by`, `agg`, `named`,
  …): `setattr(Purchase, method, staticmethod(getattr(src, method)))`.
  Now `Purchase.with_columns(...)` works without instantiating
  anything.
- Returns `Purchase`.

**Step 6.** Python re-binds `Purchase` in the user's module to the
mutated class object. Decoration done. **Nothing has crossed the
wire.**

---

## Phase C — `@bv.event def UserStats(e: Purchase)` decoration

**Step 7.** Python compiles the `def UserStats` into a function
object. **The body has not run yet.** Python then calls
`UserStats = event(UserStats)`.

**Step 8.** Inside `event` again (`_events.py:387–396`):

```python
if inspect.isclass(cls_or_fn):               # False — it's a function
    ...
return _make_event_derivation(cls_or_fn)
```

**Step 9.** `_make_event_derivation(fn=UserStats)` at `_events.py:297`:

- `sig = inspect.signature(fn)` → `Parameter('e', annotation=Purchase)`
  (or the string `'Purchase'` under `from __future__ import annotations`).
- Type-hint resolution chain (`_events.py:311–318`):
  `_collect_closure_cells_for_events(fn)` +
  `_collect_caller_frame_locals_for_events()` build a `localns`;
  `get_type_hints(fn, globalns=fn.__globals__, localns=localns)`
  resolves `e: Purchase` → the actual `Purchase` class.
- For the one parameter `e`: the resolved annotation is the
  `Purchase` class. Since `Purchase` is *not* a raw `EventDerivation`
  (the rejected case at `_events.py:329`), it is appended:
  `upstream_proxies = [Purchase]`.
- **Step 9a — the user's function body now runs:** `result =
  fn(*upstream_proxies)` (`_events.py:347`). That is `result =
  UserStats(Purchase)`. Phase D below traces what happens inside.
- After the body returns: assert `isinstance(result,
  EventDerivation)`; set `result._name = "UserStats"`; set
  `result._is_bv_event_function = True` (the marker `App.register`
  checks).
- Return `result`. Python re-binds the user's `UserStats` name to
  this `EventDerivation` instance. **`UserStats` is no longer a
  function — it's an EventDerivation.**

---

## Phase D — Inside the running user function `UserStats(e=Purchase)`

The body has two statements.

### Statement 1: `e = e.with_columns(is_big=bv.col("amount") > 100.0)`

**Step 10.** `bv.col("amount")` calls `col("amount")` at
`_col.py:189`. Returns `_Col(name="amount")` — a frozen dataclass.

**Step 11.** `_Col("amount") > 100.0` triggers Python's `__gt__`
dispatch on `_Col`, which inherits `_Expr.__gt__` at `_col.py:58`:

```python
def __gt__(self, other): return _BinOp(">", self, _coerce(other))
```

- `_coerce(100.0)` at `_col.py:184`: not an `_Expr`, so returns
  `_Literal(value=100.0)`.
- Returns `_BinOp(op=">", left=_Col("amount"), right=_Literal(100.0))`.
  **Still a Python object tree.**

**Step 12.** `e.with_columns(is_big=<_BinOp...>)` — `e` is the
`Purchase` class. `Purchase.with_columns` is the staticmethod bound
in Step 5, which forwards to `_ChainMixin.with_columns(self=src,
**{"is_big": _BinOp(...)})` at `_events.py:59`:

```python
def with_columns(self, **exprs):
    return _make_derivation(
        self,
        {"op": "with_columns",
         "exprs": {k: (v.to_expr_string() if isinstance(v, _Expr) else v)
                   for k, v in exprs.items()}},
    )
```

**Step 13.  ←  THIS IS WHERE SERIALIZATION HAPPENS.** Inside the
dict comprehension at `_events.py:65`, the `_BinOp` object's
`.to_expr_string()` is called *eagerly*:

- `_BinOp.to_expr_string()` at `_col.py:149`:

  ```python
  return f"({self.left.to_expr_string()} {self.op} {self.right.to_expr_string()})"
  ```

  - `self.left.to_expr_string()` → `_Col.to_expr_string()` at
    `_col.py:118` → `"amount"`.
  - `self.right.to_expr_string()` → `_Literal.to_expr_string()` at
    `_col.py:129` → `"100.0"` (via the final `repr(value)` branch).
  - Result: `"(amount > 100.0)"` — **a Python string.**
- The dict built is `{"op": "with_columns", "exprs": {"is_big":
  "(amount > 100.0)"}}`. **The `_BinOp` object is now garbage; only
  the string remains.**

**Step 14.** `_make_derivation(parent=src, step={...})` at
`_events.py:149`:

- `new_chain = list(src._chain) + [step]` → `[step]`.
- Returns `EventDerivation(name="Purchase__derived_1", parent=src,
  chain=new_chain)`. The `EventDerivation.__init__` at
  `_events.py:128` sets `_kind = "event_derivation"`.

**Step 15.** Python re-binds the local `e` in the body to this new
`EventDerivation`. (The class `Purchase` itself is unmodified —
`with_columns` doesn't mutate the upstream.)

### Statement 2: `return e.group_by("user_id").agg(...)`

**Step 16.** `e.group_by("user_id")` — `e` is now the
`EventDerivation`. `_ChainMixin.group_by(self=ev_deriv,
*keys=("user_id",))` at `_events.py:89` returns `GroupBy(parent=ev_deriv,
keys=("user_id",))`. **No chain mutation yet** — `GroupBy` is just
an intermediate.

**Step 17.** Python evaluates each `.agg(**named)` keyword argument
*before* the call to `agg`:

#### 17a. `bv.count(where=bv.col("is_big"), window="1h")`

- `bv.col("is_big")` → `_Col("is_big")`.
- `count(window="1h", where=_Col("is_big"))` at `_agg.py:117`:
  - `_validate_window("1h", "count", required=False)` (`_agg.py:32`)
    → OK.
  - `_serialize_where(_Col("is_big"))` at `_agg.py:76` — **second
    serialization site:** calls `.to_expr_string()` on the `_Expr`,
    returning `"is_big"` (`_Col.to_expr_string()`).
  - Returns `AggDescriptor(op="count", field=None, window="1h",
    half_life=None, extras={}, where="is_big")`.

#### 17b. `bv.sum("amount", window="24h")`

- `sum(field="amount", window="24h", where=None)` at `_agg.py:123`:
  - `_enforce_field_str("amount", "sum")` (`_agg.py:51`) — `"amount"`
    is a `str`, not an `_Expr`, → OK.
  - `_validate_window("24h", "sum", required=False)` → OK.
  - `_serialize_where(None)` returns `None` immediately.
  - Returns `AggDescriptor(op="sum", field="amount", window="24h",
    where=None)`.

#### 17c. `bv.count(where=bv.col("item").isnull() == False, window="24h")`

- `bv.col("item")` → `_Col("item")`.
- `.isnull()` — `_Expr.isnull` at `_col.py:99` →
  `_UnaryOp(op="isnull", operand=_Col("item"))`.
- `<_UnaryOp> == False` — `_Expr.__eq__` at `_col.py:70` →
  `_BinOp("==", _UnaryOp(...), _coerce(False))`. `_coerce(False)`
  → `_Literal(False)`.
- `count(window="24h", where=<_BinOp>)` → `_serialize_where` calls
  `to_expr_string()`:
  - `_BinOp.to_expr_string()` recurses:
    - `left.to_expr_string()` is `_UnaryOp.to_expr_string()` at
      `_col.py:161`. Branch `op == "isnull"`: `f"({self.operand.to_expr_string()}
      == null)"` → `"(item == null)"`.
    - `right.to_expr_string()` is `_Literal(False).to_expr_string()`
      at `_col.py:129`. Branch `isinstance(value, bool)`: returns
      `"false"`.
  - Final: `"((item == null) == false)"`.
- Returns `AggDescriptor(op="count", window="24h", where="((item ==
  null) == false)")`.

**Step 18.** Now `GroupBy.agg(big_count_1h=<descriptor>,
total_24h=<descriptor>, named_24h=<descriptor>)` at `_events.py:162`
runs:

```python
new_chain = list(self._parent._chain) + [{
    "op": "agg",
    "keys": list(self._keys),
    "aggs": {name: (agg.to_dict() if hasattr(agg, "to_dict") else agg)
             for name, agg in named.items()},
}]
```

**Step 19. ← THIRD SERIALIZATION SITE.** Each `AggDescriptor.to_dict()`
runs at `_agg.py:103`:

- For `big_count_1h`: `{"op": "count", "window": "1h", "where": "is_big"}`.
- For `total_24h`: `{"op": "sum", "field": "amount", "window": "24h"}`.
- For `named_24h`: `{"op": "count", "window": "24h", "where": "((item
  == null) == false)"}`.

(The `to_dict` skips keys whose value is `None` — that's why
`total_24h` has no `where`.)

**Step 20.** Final EventDerivation built (`_events.py:178-185`):

```python
d = EventDerivation(name="Purchase__derived_1__agg",
                    parent=<prev EventDerivation>,
                    chain=[<with_columns step>, <agg step>])
d._kind     = "aggregation"
d._key_cols = ["user_id"]
return d
```

**Step 21.** The body's `return` returns this final `EventDerivation`.
Control returns to Step 9a (the `result = fn(*upstream_proxies)`
call in `_make_event_derivation`). Step 9 finishes: sets `_name =
"UserStats"`, `_is_bv_event_function = True`, returns. Python
rebinds the user's `UserStats` name to this `EventDerivation`.

---

## Phase E — `app.register(Purchase, UserStats)`

**Step 22.** `bv.App(...)` constructs an `App` (transport setup; not
relevant to the JSON flow).

**Step 23.** `app.register(Purchase, UserStats)` at `_app.py:533`:

- Loops over descriptors and rejects any raw `EventDerivation` that
  lacks `_is_bv_event_function`. Both pass.
- `t = self._require_transport()`.
- **`payload = _to_register_json((Purchase, UserStats), force=False,
  dry_run=False)`.**
- Sends payload; returns server response.

**Step 24.** `_to_register_json(descriptors=(Purchase, UserStats), …)`
at `_app.py:26`:

```python
nodes = []
for d in descriptors:
    node = _descriptor_to_node(d)
    if node is None: continue
    nodes.append(node)
payload = {"nodes": nodes}
return json.dumps(payload, ensure_ascii=False).encode("utf-8")
```

**Step 25.** Per-descriptor reshaping in `_descriptor_to_node`
(`_app.py:52`):

### 25a. `Purchase` (kind == `"event_source"`)

Branch at `_app.py:64`:

- `_python_type_to_wire(str)` → `"str"`, `_python_type_to_wire(float)`
  → `"f64"`, etc.
- Returns:

  ```python
  {"kind": "event", "name": "Purchase",
   "schema": {"fields": {"user_id": "str", "amount": "f64",
                         "item": "str", "ts": "i64"},
              "optional_fields": []}}
  ```

### 25b. `UserStats` (kind == `"aggregation"`)

Branch at `_app.py:138`:

- Walks `_parent` pointers up to the root (`Purchase`'s
  `EventSource`); takes its `_name` → `upstreams = ["Purchase"]`.
- `ops = _chain_to_ops(d._chain)` (`_app.py:403`):
  - Step `{"op": "with_columns", "exprs": {"is_big": "(amount > 100.0)"}}`
    is passed through unchanged (the catch-all branch at line 430).
  - Step `{"op": "agg", ...}` is **rewritten** to `{"op": "group_by",
    "keys": [...], "agg": {name: {"op": op_name, "params": {...other
    keys...}}}}`. The `agg → group_by` rename + `params` repackaging
    is the only structural transform `_app.py` performs.
- `has_agg` is True → `output_kind = "table"`.
- `_infer_derivation_schema(...)` builds a `{column: type}` map from
  `_FIXED_OP_OUTPUT_TYPE` (`_app.py:194`) — `count → "i64"`,
  `sum → "f64"`.
- Returns:

  ```python
  {"kind": "derivation",
   "name": "UserStats",
   "output_kind": "table",
   "upstreams": ["Purchase"],
   "ops": [
     {"op": "with_columns", "exprs": {"is_big": "(amount > 100.0)"}},
     {"op": "group_by", "keys": ["user_id"],
      "agg": {
        "big_count_1h": {"op": "count",
                         "params": {"window": "1h", "where": "is_big"}},
        "total_24h":    {"op": "sum",
                         "params": {"field": "amount", "window": "24h"}},
        "named_24h":    {"op": "count",
                         "params": {"window": "24h",
                                    "where": "((item == null) == false)"}},
      }},
   ],
   "schema": {"fields": {"big_count_1h": "i64", "total_24h": "f64",
                         "named_24h": "i64"},
              "optional_fields": []}}
  ```

**Step 26.** `_to_register_json` wraps both nodes into `{"nodes":
[<event>, <derivation>]}`, calls `json.dumps(payload,
ensure_ascii=False).encode("utf-8")`, returns the bytes.

**Step 27.** Back in `App.register` at `_app.py:582`:
`t.send_register(payload)` hands those bytes to the transport, which
prepends the wire frame envelope (`OP_REGISTER = 0x0001`,
`CT_JSON = 0x01`, length prefix) via `encode_frame` at `_wire.py:78`
and writes to the socket.

---

## Where are all the `_Expr` objects?

| Lifecycle                                | What exists                                                                 |
|------------------------------------------|-----------------------------------------------------------------------------|
| Module load (Step 1)                     | Only function definitions in memory.                                        |
| `@bv.event class` (Steps 2–6)            | `Purchase` class has `_chain=[]`. No `_Expr` anywhere.                      |
| `@bv.event def`, while body runs (9a–21) | `_Expr` trees are momentarily alive — for the duration of *one expression*. |
| After `@bv.event def` returns (Step 21)  | `UserStats._chain` is a list of dicts of strings. **No `_Expr` exists.**    |
| `app.register(...)` (Steps 23–27)        | `_app.py` reshapes those dicts and bundles them. Never sees an `_Expr`.     |
| JSON bytes on the wire                   | Strings inside dicts inside `{"nodes": [...]}`.                             |

**There are exactly three lines in the entire codebase that turn
`_Expr` into a string:**

```
_events.py:46    "expr":  expr.to_expr_string()           # filter
_events.py:65    {k: (v.to_expr_string() if ...)}         # with_columns
_agg.py:80       return where.to_expr_string()            # _serialize_where → agg.where=
```

Every wire string in the register payload above came from one of
those three lines. Everything before is operator-overload
accumulation; everything after is dict reshaping and `json.dumps`.

---

## Why this matters for RFC 0001

The proposed `@bv.expr` decorator slots in at exactly the same
boundary. It produces a `_Sym*` tree, calls `.to_expr_string()`
*once* at decoration time, and from that point on the IR is a string
indistinguishable from one that `_col.py` would have produced.

That means the entire Phase E flow above is unchanged — `_app.py`
keeps seeing dicts of strings; `_to_register_json` keeps emitting
the same envelope; the wire frame layout is identical. The only
change is that some of those strings are now allowed to contain
`(if ... then ... else ...)`, `(let ... = ... in ...)`, and
`(a < b < c)` forms that the current parser would reject.

That's why the RFC can claim "no new wire opcodes": the opcode set
is the *frame* protocol (Step 27), and the *expression-string*
grammar is one layer below that (touched only inside the three
lines above). Only the latter grows.

---

## File reference

| Step                            | File                                                     |
|---------------------------------|----------------------------------------------------------|
| `event` decorator entry         | `python/beava/_events.py:359`                            |
| `_make_event_source`            | `python/beava/_events.py:208`                            |
| `_make_event_derivation`        | `python/beava/_events.py:297`                            |
| `_ChainMixin.with_columns`      | `python/beava/_events.py:59`                             |
| `_ChainMixin.group_by`          | `python/beava/_events.py:89`                             |
| `_ChainMixin.filter`            | `python/beava/_events.py:46`                             |
| `_make_derivation`              | `python/beava/_events.py:149`                            |
| `GroupBy.agg`                   | `python/beava/_events.py:162`                            |
| `bv.col` / `bv.lit`             | `python/beava/_col.py:189`, `:202`                       |
| `_Expr.__gt__` (and siblings)   | `python/beava/_col.py:58–90`                             |
| `_Expr.isnull`                  | `python/beava/_col.py:99`                                |
| `_Col.to_expr_string`           | `python/beava/_col.py:118`                               |
| `_Literal.to_expr_string`       | `python/beava/_col.py:129`                               |
| `_BinOp.to_expr_string`         | `python/beava/_col.py:149`                               |
| `_UnaryOp.to_expr_string`       | `python/beava/_col.py:161`                               |
| `_coerce`                       | `python/beava/_col.py:184`                               |
| `count` / `sum` helpers         | `python/beava/_agg.py:117`, `:123`                       |
| `_enforce_field_str`            | `python/beava/_agg.py:51`                                |
| `_serialize_where`              | `python/beava/_agg.py:76`                                |
| `AggDescriptor.to_dict`         | `python/beava/_agg.py:103`                               |
| `App.register`                  | `python/beava/_app.py:533`                               |
| `_to_register_json`             | `python/beava/_app.py:26`                                |
| `_descriptor_to_node`           | `python/beava/_app.py:52`                                |
| `_chain_to_ops`                 | `python/beava/_app.py:403`                               |
| `_FIXED_OP_OUTPUT_TYPE`         | `python/beava/_app.py:194`                               |
| `_python_type_to_wire`          | `python/beava/_app.py:174`                               |
| `encode_frame` (wire envelope)  | `python/beava/_wire.py:78`                               |
