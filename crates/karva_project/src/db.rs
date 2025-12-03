use crate::{Project, System};

pub trait Db {
    fn system(&self) -> &dyn System;
    fn project(&self) -> &Project;
}
