//! Authorization with Cedar (ADR-0006, ADR-0014): the schema and policies in `policies/`,
//! validated strictly when the server starts and in the tests; agent grants as links of
//! the grant templates, each under its own policy id.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use anyhow::{Context as _, Result, anyhow, bail};
use cedar_policy::{
    Authorizer, Context, Decision, Entities, Entity, EntityId, EntityTypeName, EntityUid, PolicyId, PolicySet, Request,
    RestrictedExpression, Schema, SlotId, ValidationMode, Validator,
};

const SCHEMA: &str = include_str!("../policies/nitya.cedarschema");
const POLICIES: &str = include_str!("../policies/nitya.cedar");
const GRANTS: &str = include_str!("../policies/grants.cedar");

/// Who is asking: a device acting as its user, or an agent job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    Device { id: String, user: String, kind: String },
    Agent { id: String, user: String },
}

impl Principal {
    /// The principal a certificate names, by its device key, which renewals keep: agent
    /// certificates are agents, the rest devices.
    pub fn of(cert: &nitya_proto::v1::CertBody) -> Principal {
        let (id, user, kind) = (nitya_device::hex(&cert.device_key), cert.account.clone(), cert.kind.as_str());
        if kind == "agent" { Principal::Agent { id, user } } else { Principal::Device { id, user, kind: kind.into() } }
    }
}

/// What is asked about. Each resource names its account; items name their source.
/// Plans, sources and items are asked about once plans and sync land (phases 2 and 5).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum Resource {
    KeySet { user: String },
    AuditLog { user: String },
    Plan { id: String, user: String },
    Source { id: String, user: String, class: String },
    Item { id: String, source: String, user: String, class: String },
}

/// An agent's grant of one action on one source (or a whole account's resource).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub id: String,
    pub agent: String,
    pub action: String,
    pub source: String,
}

/// The decision and the ids of the policies that made it, for the audit entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub allowed: bool,
    pub policies: Vec<String>,
}

pub struct Policy {
    schema: Schema,
    policies: PolicySet,
}

fn uid(ty: &str, id: &str) -> Result<EntityUid> {
    Ok(EntityUid::from_type_name_and_id(EntityTypeName::from_str(&format!("Nitya::{ty}"))?, EntityId::new(id)))
}

fn entity(u: EntityUid, attrs: &[(&str, RestrictedExpression)], parents: &[EntityUid]) -> Result<Entity> {
    let attrs: HashMap<String, RestrictedExpression> = attrs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    Ok(Entity::new(u, attrs, parents.iter().cloned().collect::<HashSet<_>>())?)
}

impl Policy {
    /// The schema and policies, with `grants` linked; refused unless every policy and
    /// template validates strictly against the schema.
    pub fn new(grants: &[Grant]) -> Result<Policy> {
        let (schema, _) = Schema::from_cedarschema_str(SCHEMA).map_err(|e| anyhow!("the Cedar schema: {e}"))?;
        let mut policies = PolicySet::new();
        for p in PolicySet::from_str(POLICIES).map_err(|e| anyhow!("nitya.cedar: {e}"))?.policies() {
            let id = p.annotation("id").context("a policy without @id")?.to_string();
            policies.add(p.new_id(PolicyId::new(id)))?;
        }
        for t in PolicySet::from_str(GRANTS).map_err(|e| anyhow!("grants.cedar: {e}"))?.templates() {
            let id = t.annotation("id").context("a template without @id")?.to_string();
            policies.add_template(t.new_id(PolicyId::new(id)))?;
        }
        let v = Validator::new(schema.clone()).validate(&policies, ValidationMode::Strict);
        if !v.validation_passed() {
            bail!("the policies do not validate against the schema: {}", v.validation_errors().map(|e| e.to_string()).collect::<Vec<_>>().join("; "));
        }
        let mut p = Policy { schema, policies };
        for g in grants {
            p.link(g)?;
        }
        Ok(p)
    }

    fn link(&mut self, g: &Grant) -> Result<()> {
        let vals = HashMap::from([(SlotId::principal(), uid("Agent", &g.agent)?), (SlotId::resource(), uid("Source", &g.source)?)]);
        self.policies
            .link(PolicyId::new(format!("grant:{}", g.action)), PolicyId::new(format!("grant-{}", g.id)), vals)
            .with_context(|| format!("grant {}: no grant action {}", g.id, g.action))
    }

    /// Whether `principal` may do `action` on `resource`; `plan_approved` is the context of
    /// a write (the server sets it only after checking the plan's hash).
    pub fn decide(&self, principal: &Principal, action: &str, resource: &Resource, plan_approved: bool) -> Result<Verdict> {
        let mut ents = Vec::new();
        let p = match principal {
            Principal::Device { id, user, kind } => {
                let u = uid("User", user)?;
                ents.push(entity(u.clone(), &[], &[])?);
                let d = uid("Device", id)?;
                ents.push(entity(d.clone(), &[("user", RestrictedExpression::new_entity_uid(u.clone())), ("kind", RestrictedExpression::new_string(kind.clone()))], &[u])?);
                d
            }
            Principal::Agent { id, user } => {
                let u = uid("User", user)?;
                ents.push(entity(u.clone(), &[], &[])?);
                let a = uid("Agent", id)?;
                ents.push(entity(a.clone(), &[("user", RestrictedExpression::new_entity_uid(u.clone()))], &[u])?);
                a
            }
        };
        let r = match resource {
            Resource::KeySet { user } => {
                let u = uid("User", user)?;
                let k = uid("KeySet", user)?;
                ents.extend([entity(u.clone(), &[], &[])?, entity(k.clone(), &[], &[u])?]);
                k
            }
            Resource::AuditLog { user } => {
                let u = uid("User", user)?;
                let l = uid("AuditLog", user)?;
                ents.extend([entity(u.clone(), &[], &[])?, entity(l.clone(), &[], &[u])?]);
                l
            }
            Resource::Plan { id, user } => {
                let u = uid("User", user)?;
                let k = uid("Plan", id)?;
                ents.extend([entity(u.clone(), &[], &[])?, entity(k.clone(), &[], &[u])?]);
                k
            }
            Resource::Source { id, user, class } => {
                let u = uid("User", user)?;
                let s = uid("Source", id)?;
                ents.extend([entity(u.clone(), &[], &[])?, entity(s.clone(), &[("class", RestrictedExpression::new_string(class.clone()))], &[u])?]);
                s
            }
            Resource::Item { id, source, user, class } => {
                let u = uid("User", user)?;
                let s = uid("Source", source)?;
                let i = uid("Item", id)?;
                ents.extend([
                    entity(u.clone(), &[], &[])?,
                    entity(s.clone(), &[("class", RestrictedExpression::new_string(class.clone()))], &[u])?,
                    entity(i.clone(), &[], &[s])?,
                ]);
                i
            }
        };
        // The same user may appear twice (principal and resource): keep one.
        let mut seen = HashSet::new();
        ents.retain(|e| seen.insert(e.uid()));
        let entities = Entities::from_entities(ents, Some(&self.schema))?;
        let a = uid("Action", action)?;
        let ctx = if self.is_write(action) {
            Context::from_pairs([("plan_approved".to_string(), RestrictedExpression::new_bool(plan_approved))])?
        } else {
            Context::empty()
        };
        let req = Request::new(p, a, r, ctx, Some(&self.schema))?;
        let resp = Authorizer::new().is_authorized(&req, &self.policies, &entities);
        let mut policies: Vec<String> = resp.diagnostics().reason().map(|id| id.to_string()).collect();
        policies.sort();
        if let Some(e) = resp.diagnostics().errors().next() {
            bail!("evaluating the policies: {e}");
        }
        Ok(Verdict { allowed: resp.decision() == Decision::Allow, policies })
    }

    fn is_write(&self, action: &str) -> bool {
        matches!(action, "calendar.write" | "calendar.invite" | "draft.create" | "task.write")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cert(user: &str, kind: &str) -> nitya_proto::v1::CertBody {
        nitya_proto::v1::CertBody { account: user.into(), kind: kind.into(), device_key: vec![1; 32], ..Default::default() }
    }
    fn device(user: &str) -> Principal {
        Principal::of(&cert(user, "node"))
    }
    fn agent(id: &str) -> Principal {
        Principal::Agent { id: id.into(), user: "ank".into() }
    }
    fn cal(user: &str) -> Resource {
        Resource::Source { id: format!("{user}/google/calendar/primary"), user: user.into(), class: "calendar".into() }
    }
    fn grant(id: &str, action: &str) -> Grant {
        Grant { id: id.into(), agent: "job-1".into(), action: action.into(), source: "ank/google/calendar/primary".into() }
    }

    #[test]
    fn the_policies_validate_strictly_against_the_schema() {
        Policy::new(&[]).unwrap();
    }

    #[test]
    fn a_device_acts_as_its_user_on_its_own_account_only() {
        let p = Policy::new(&[]).unwrap();
        let v = p.decide(&device("ank"), "keys.write", &Resource::KeySet { user: "ank".into() }, false).unwrap();
        assert_eq!(v, Verdict { allowed: true, policies: vec!["devices-act-as-their-user".into()] });
        assert!(p.decide(&device("ank"), "calendar.write", &cal("ank"), false).unwrap().allowed);
        assert!(p.decide(&Principal::of(&cert("ank", "app")), "plan.approve", &Resource::Plan { id: "p1".into(), user: "ank".into() }, false).unwrap().allowed);
        let other = p.decide(&device("ank"), "calendar.read", &cal("someone"), false).unwrap();
        assert_eq!(other, Verdict { allowed: false, policies: vec!["only-the-callers-own-account".into()] });
        assert!(!p.decide(&device("ank"), "keys.read", &Resource::KeySet { user: "someone".into() }, false).unwrap().allowed);
    }

    #[test]
    fn an_agent_has_only_its_grants_and_no_grant_implies_another() {
        let p = Policy::new(&[grant("g1", "calendar.write")]).unwrap();
        // Write does not imply read.
        assert!(!p.decide(&agent("job-1"), "calendar.read", &cal("ank"), false).unwrap().allowed);
        let v = p.decide(&agent("job-1"), "calendar.write", &cal("ank"), true).unwrap();
        assert_eq!(v, Verdict { allowed: true, policies: vec!["grant-g1".into()] });
        // Another agent, or another source, gets nothing from it.
        assert!(!p.decide(&agent("job-2"), "calendar.write", &cal("ank"), true).unwrap().allowed);
        let mail = Resource::Source { id: "ank/google/mail/inbox".into(), user: "ank".into(), class: "mail".into() };
        assert!(!p.decide(&agent("job-1"), "calendar.write", &mail, true).unwrap().allowed);
        // A grant on a source covers its items.
        let p = Policy::new(&[grant("g2", "calendar.event.read")]).unwrap();
        let item = Resource::Item { id: "e1".into(), source: "ank/google/calendar/primary".into(), user: "ank".into(), class: "calendar".into() };
        assert!(p.decide(&agent("job-1"), "calendar.event.read", &item, false).unwrap().allowed);
    }

    #[test]
    fn an_agent_write_needs_an_approved_plan() {
        let p = Policy::new(&[grant("g1", "calendar.write"), grant("g2", "draft.create")]).unwrap();
        let v = p.decide(&agent("job-1"), "calendar.write", &cal("ank"), false).unwrap();
        assert_eq!(v, Verdict { allowed: false, policies: vec!["agent-writes-need-an-approved-plan".into()] });
        assert!(!p.decide(&agent("job-1"), "draft.create", &cal("ank"), false).unwrap().allowed);
        assert!(p.decide(&agent("job-1"), "draft.create", &cal("ank"), true).unwrap().allowed);
    }

    #[test]
    fn an_agent_never_holds_keys_or_approves_plans() {
        let p = Policy::new(&[]).unwrap();
        let ks = Resource::KeySet { user: "ank".into() };
        assert!(!p.decide(&agent("job-1"), "keys.read", &ks, false).unwrap().allowed);
        assert!(!p.decide(&Principal::of(&cert("ank", "agent")), "keys.write", &ks, false).unwrap().allowed);
        assert!(!p.decide(&agent("job-1"), "plan.approve", &Resource::Plan { id: "p1".into(), user: "ank".into() }, false).unwrap().allowed);
        // Nor reads the audit log: no policy permits it.
        assert!(!p.decide(&agent("job-1"), "audit.read", &Resource::AuditLog { user: "ank".into() }, false).unwrap().allowed);
        assert!(p.decide(&device("ank"), "audit.read", &Resource::AuditLog { user: "ank".into() }, false).unwrap().allowed);
    }

    #[test]
    fn a_grant_for_an_unknown_action_is_refused() {
        assert!(Policy::new(&[grant("g1", "mail.send")]).is_err());
    }
}
