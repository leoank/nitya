// One template per grant action (ADR-0004, ADR-0014): a grant links one to an agent and
// a source, under the grant's own policy id. The server links only unexpired grants.

@id("grant:calendar.read")
permit (principal == ?principal, action == Nitya::Action::"calendar.read", resource in ?resource);

@id("grant:calendar.event.read")
permit (principal == ?principal, action == Nitya::Action::"calendar.event.read", resource in ?resource);

@id("grant:calendar.write")
permit (principal == ?principal, action == Nitya::Action::"calendar.write", resource in ?resource);

@id("grant:calendar.invite")
permit (principal == ?principal, action == Nitya::Action::"calendar.invite", resource in ?resource);

@id("grant:mail.search")
permit (principal == ?principal, action == Nitya::Action::"mail.search", resource in ?resource);

@id("grant:mail.read")
permit (principal == ?principal, action == Nitya::Action::"mail.read", resource in ?resource);

@id("grant:draft.create")
permit (principal == ?principal, action == Nitya::Action::"draft.create", resource in ?resource);

@id("grant:task.read")
permit (principal == ?principal, action == Nitya::Action::"task.read", resource in ?resource);

@id("grant:task.write")
permit (principal == ?principal, action == Nitya::Action::"task.write", resource in ?resource);
