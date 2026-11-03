### Assemblies

- Keep groups whose joints already close at their stored poses before placing other joined parts, and prefer a non-identity stored world pose when starting an unplaced joint group.
- Carry the smallest connected holder or its origin body assembly when a joint places it, leaving unrelated children of identity organisers where they belong.
- Resolve isolated child pose markers within a posed component against already closing joints, while preserving explicit holder and independently contained poses.
