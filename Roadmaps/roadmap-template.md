# [project] roadmap

## Objective

[One or two sentences defining the finished user-visible outcome and explicit
scope boundaries.]

## Rules

- Complete phases strictly in order.
- Keep each phase small enough to implement and verify independently.
- Preserve [required behavior/contract] unless an intentional change is
  documented and tested.
- Exclude [out-of-scope work].

## Sequential phases

Choose the number of phases from the work itself. Use only as many as needed;
some roadmaps may have three phases, others ten. Every phase must depend on the
previous one and leave a small, verifiable outcome.

### Phase [N] — [short outcome]

- [ ] [First small, concrete step that makes this phase true.]
- [ ] [Next dependent step; omit when it does not add value.]
- [ ] [Focused proof for this phase; combine with implementation when natural.]

**Exit:** [A short observable condition proving this phase is complete.]

Repeat the phase pattern only for actual sequential work. Do not add filler
phases, repeated checklists, or a separate testing phase when verification fits
inside the relevant phase.

Add a final audit only when it is materially broader than the checks already
inside the phases. It is optional, not a required final phase.
