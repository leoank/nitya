// nitya's policies (ADR-0006, ADR-0014). Every decision names the policies that made it,
// by their @id, in its audit entry (ADR-0007).

@id("devices-act-as-their-user")
permit (principal is Nitya::Device, action, resource)
when { resource in principal.user };

@id("only-the-callers-own-account")
forbid (principal, action, resource)
unless { resource in principal.user };

@id("agents-never-touch-keys")
forbid (principal is Nitya::Agent, action in [Nitya::Action::"keys.read", Nitya::Action::"keys.write"], resource);

@id("agents-never-approve-plans")
forbid (principal is Nitya::Agent, action == Nitya::Action::"plan.approve", resource);

@id("agent-writes-need-an-approved-plan")
forbid (principal is Nitya::Agent, action in Nitya::Action::"writes", resource)
unless { context.plan_approved };
