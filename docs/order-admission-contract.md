# Order Admission Contract

_Between Order Gateway and Matching Engine_

## 1. Purpose and Scope

This document defines the **Order Admission Contract** between the Order Gateway and the Matching Engine.
It specifies responsibility boundaries, delivery semantics, acknowledgment rules, and failure handling for order admission in a distributed trading system.

The goal of this contract is to ensure that order flow remains **correct, recoverable, and evolvable** under high concurrency, partial failures, and network unreliability.

This contract governs only the **Gateway → Matching Engine** interaction. It does not define matching algorithms, settlement logic, or downstream persistence guarantees.

---

## 2. System Roles and Assumptions

The Order Gateway acts as the system ingress, responsible for protocol adaptation, syntactic validation, and flow control.
The Matching Engine is the first **authoritative processing component** in the order lifecycle and determines whether an order is accepted into the system.

The system operates under the following assumptions:

- Both Gateway and Matching Engine are horizontally scalable and stateless by design
- Network communication is unreliable and may exhibit loss, duplication, or reordering
- Clients may retry requests arbitrarily
- Correctness is prioritized over throughput

---

## 3. Authority of Order Admission

**Successful forwarding by the Gateway does not imply order acceptance.**

An order is considered admitted into the system only after an explicit acknowledgment (ACK) from the Matching Engine.
Prior to that point, the order remains in an _Unconfirmed_ state and carries no system-level guarantees.

The Gateway does not own the authoritative state of an order. Its responsibility ends at reliable delivery and faithful propagation of downstream responses.

---

## 4. Acknowledgment Semantics

The Matching Engine must respond to each order submission with a clear admission signal, expressing one of the following outcomes:

- **Accepted**: the order has been admitted and ownership is transferred to the Matching Engine
- **Rejected**: the order has been definitively refused due to non-recoverable reasons
- **Overloaded / Unavailable**: the Matching Engine cannot accept the order at this time

An acknowledgment represents **responsibility transfer**, not business completion.
Once an order is acknowledged as Accepted, its lifecycle is fully owned by the Matching Engine.

---

## 5. Delivery Semantics and Idempotency

The Gateway delivers orders to the Matching Engine using **at-least-once** semantics.

To prevent duplicate processing, the system defines a canonical idempotency key:

> `(principal_id, nonce)`

The Matching Engine must guarantee idempotent handling of requests within its idempotency window.
The Gateway must ensure that its retry behavior does not exceed this window.

Duplicate submissions with the same idempotency key must result in consistent outcomes.

---

下面是保持**白皮书语气、克制承诺、法律/工程都能站得住**的一版英文翻译。我刻意避免了过度技术化的句法，同时把“语义边界”说得很清楚，适合直接放进 design docs 或 README 的规范章节。

---

## 6. Ordering and Out-of-Order Handling

The Gateway provides **no guarantees** regarding the ordering of order requests.
The order in which requests are received, validated, or forwarded by the Gateway has **no semantic causal relationship** with the order in which they are processed or matched by the Matching Engine.

Under multi-instance Gateway deployments, network jitter, retries, and concurrent scheduling, requests may arrive at the Matching Engine in arbitrary order. The Gateway does not maintain any cross-instance global timeline, nor does it attempt to sort, delay, or reorder requests. As such, the Gateway cannot—and must not—be treated as a time authority within the system.

The Matching Engine is the **sole time authority** for order processing. The notion of “first arrival” is defined strictly by the moment an order is actually received by the Matching Engine and admitted into its internal processing pipeline. The Matching Engine cannot and does not attempt to reason about the existence of orders that are logically earlier but still in transit or not yet delivered. Once a request arrives, it is considered a visible event at that point in time.

If the system requires ordering, fairness, or deterministic processing guarantees (for example, strict time-priority, sequence numbers, or nonce-based ordering), such guarantees must be implemented explicitly within the Matching Engine based on declared fields and its own execution model. Ordering semantics are neither implicitly provided by the Gateway nor within the Gateway’s responsibility boundary.

> **Time Authority**
>
> The Time Authority refers to the single component or stage in the system that is recognized as authoritative for determining the temporal ordering of events. In this system, time authority is not derived from the Gateway, client-side timestamps, or network arrival order. Instead, it is established exclusively by the moment an order is received and admitted into the Matching Engine’s internal processing flow. Any temporal information prior to that point—including client timestamps, Gateway receive times, or relative ordering across Gateway instances—is treated as reference metadata only and carries no ordering semantics. Centralizing time authority avoids introducing unverifiable or unmaintainable ordering guarantees in a distributed environment, thereby preserving determinism and interpretability in the matching logic.

---

## 7. Failure Model and Error Classification

Failures returned by the Matching Engine must be explicitly classified:

- **Non-recoverable failures** (e.g., invalid parameters, business rule violations)
  These failures terminate the request lifecycle and must not be retried.

- **Recoverable failures** (e.g., timeouts, overload, temporary unavailability)
  These failures may be retried by the client through the Gateway.

The Gateway must not infer failure semantics or perform implicit retries on behalf of downstream components.

---

## 8. Backpressure and Overload Signaling

The Matching Engine is the source of truth for system capacity.

When overloaded, the Matching Engine must explicitly signal its inability to accept new orders.
Upon receiving such signals, the Gateway must fail fast rather than buffering unbounded requests.

Gateway-side queues may absorb short-lived spikes but must not serve as long-term backpressure mechanisms.

---

## 9. Gateway State Constraints

The Gateway may maintain the following classes of state:

- Short-lived, best-effort caches (e.g., replay protection)
- Rate-limiting and flow-control state
- Protocol-level transient context

The Gateway must not maintain:

- Authoritative order state
- Cross-instance consistent business state
- Durable or recoverable domain data

Any state requiring strong consistency or durability must reside downstream.

---

## 10. Contract Evolution Principles

This contract may be extended in a backward-compatible manner but must not introduce ambiguous or weakened semantics.

All future changes must preserve:

- Predictable Gateway behavior
- Clear Matching Engine responsibility
- Stable client retry semantics

---

## Conclusion

The Order Admission Contract exists to preserve architectural clarity as system complexity grows.

When strictly adhered to, it enables:

- Horizontal scalability
- Fault tolerance
- Clear failure attribution
- Long-term evolvability

This document is not written for the current implementation alone, but as a **shared long-term invariant** for future maintainers and system designers.
