use crate::{mutate_state, TaskType};
use candid::{CandidType, Deserialize, Principal};
use serde::Serialize;
use std::marker::PhantomData;

const MAX_CONCURRENT: usize = 100;

#[must_use]
pub struct GuardPrincipal {
    principal: Principal,
    _marker: PhantomData<GuardPrincipal>,
}

#[derive(Debug, PartialEq, Eq, CandidType, Serialize, Deserialize)]
pub enum GuardError {
    AlreadyProcessing,
    TooManyConcurrentRequests,
}

impl GuardPrincipal {
    pub fn new(principal: Principal) -> Result<Self, GuardError> {
        mutate_state(|s| {
            if s.principal_guards.contains(&principal) {
                return Err(GuardError::AlreadyProcessing);
            }
            if s.principal_guards.len() >= MAX_CONCURRENT {
                return Err(GuardError::TooManyConcurrentRequests);
            }
            s.principal_guards.insert(principal);
            Ok(Self {
                principal,
                _marker: PhantomData,
            })
        })
    }
}

impl Drop for GuardPrincipal {
    fn drop(&mut self) {
        mutate_state(|s| s.principal_guards.remove(&self.principal));
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum TaskGuardError {
    AlreadyProcessing,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TaskGuard {
    task: TaskType,
}

impl TaskGuard {
    pub fn new(task: TaskType) -> Result<Self, TaskGuardError> {
        mutate_state(|s| {
            if !s.active_tasks.insert(task) {
                return Err(TaskGuardError::AlreadyProcessing);
            }
            Ok(Self { task })
        })
    }
}

impl Drop for TaskGuard {
    fn drop(&mut self) {
        mutate_state(|s| {
            s.active_tasks.remove(&self.task);
        });
    }
}
