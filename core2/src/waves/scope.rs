use crate::waves::{ScopeId, Variable};
use std::collections::BTreeMap;

pub const TOP_SCOPE: ScopeId = ScopeId(0);

#[derive(Debug, Clone)]
pub enum ScopesError {
    InvalidParent,
}

struct InnerScope {
    name: String,
    children: Vec<ScopeId>,
    variables: Vec<Variable>,
}

pub struct Scope<'a> {
    pub this: ScopeId,
    pub name: &'a str,
    pub children: &'a [ScopeId],
    pub variables: &'a [Variable],
}

pub struct Scopes {
    scopes: BTreeMap<ScopeId, InnerScope>,
    tops: Vec<ScopeId>,
}

impl Scopes {
    pub fn new() -> Self {
        Self {
            scopes: BTreeMap::new(),
            tops: vec![],
        }
    }

    pub fn add_scope(
        &mut self,
        parent: ScopeId,
        id: ScopeId,
        name: String,
    ) -> Result<(), ScopesError> {
        if parent >= id {
            return Err(ScopesError::InvalidParent);
        }

        if parent == TOP_SCOPE {
            self.tops.push(id);
        } else {
            let scope = self
                .scopes
                .get_mut(&parent)
                .ok_or(ScopesError::InvalidParent)?;
            scope.children.push(id);
        }

        self.scopes.insert(
            id,
            InnerScope {
                name,
                children: vec![],
                variables: vec![],
            },
        );

        Ok(())
    }

    pub fn add_variable(&mut self, scope: ScopeId, variable: Variable) -> Result<(), ScopesError> {
        let scope = self
            .scopes
            .get_mut(&scope)
            .ok_or(ScopesError::InvalidParent)?;
        scope.variables.push(variable);
        Ok(())
    }

    pub fn top_level(&self) -> impl Iterator<Item = Scope<'_>> {
        self.tops.iter().map(move |&id| self.get(id))
    }

    pub fn get(&self, id: ScopeId) -> Scope {
        let scope = &self.scopes[&id];
        Scope {
            this: id,
            name: &scope.name,
            children: &scope.children,
            variables: &scope.variables,
        }
    }
}
