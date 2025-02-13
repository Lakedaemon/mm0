//! The mid level IR, a basic block based representation used for most optimizations.
#![allow(unused)]

use std::{ops::{Index, IndexMut}, rc::Rc};
use std::convert::{TryFrom, TryInto};
use std::mem;
use num::BigInt;
use crate::{AtomId, FileSpan, LispVal, Remap, Remapper};
use super::{Binop, IntTy, Spanned, Unop, ast::ProcKind, ast, global, hir, ty};
pub use {ast::TyVarId, ty::Lifetime};

/// A variable ID. We use a different numbering here to avoid confusion with `VarId`s from HIR.
#[derive(Clone, Copy, Debug, Default, DeepSizeOf, PartialEq, Eq, Hash)]
pub struct VarId(pub u32);

impl std::fmt::Display for VarId {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "_{}", self.0)
  }
}

impl Remap for VarId {
  type Target = Self;
  fn remap(&self, _: &mut Remapper) -> Self { *self }
}

bitflags! {
  /// Attributes on arguments in a `(struct)` dependent tuple type.
  pub struct ArgAttr: u8 {
    /// An argument is nondependent if the remainder of the type does not depend on this variable.
    const NONDEP = 1;
    /// An existential argument represents `exists x. p(x)` instead of `sigma x. p(x)`; the
    /// difference is that a witness to `exists x. p(x)` is `a` such that `a: p(x)` for some `x`,
    /// while a witness to `sigma x. p(x)` is a tuple `(x, a)` such that `a: p(x)`. Thus we cannot
    /// project out existential arguments (nor can we get the type of arguments depending on an
    /// existential argument).
    const EXISTENTIAL = 2;
    /// An singleton argument is a special case where an existential argument can support
    /// projections, because it has a singleton type (for example `()`, `sn x`, or a proposition).
    const SINGLETON = 4;
    /// A ghost argument is one that has no bit-representation; a representative of
    /// `sigma x: ghost T. p(x)` is just a representative of `p(x)`, while a representative of
    /// `sigma x: T. p(x)` is the concatenation of a representative of `T` and a representative of
    /// `p(x)`. (In other words, this is like `EXISTENTIAL` but at the computation level instead of
    /// the logical level.)
    const GHOST = 8;
  }
}
crate::deep_size_0!(ArgAttr);

impl Remap for ArgAttr {
  type Target = Self;
  fn remap(&self, _: &mut Remapper) -> Self { *self }
}

/// An argument in a struct (dependent tuple).
#[derive(Debug, DeepSizeOf)]
pub struct Arg {
  /// Extra properties of the binding
  pub attr: ArgAttr,
  /// The variable to bind
  pub var: VarId,
  /// The type of the variable
  pub ty: Ty,
}

impl Remap for Arg {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self {
    Self {
      attr: self.attr,
      var: self.var,
      ty: self.ty.remap(r),
    }
  }
}

/// The type of embedded MM0 expressions.
pub type Mm0Expr = global::Mm0Expr<Expr>;

/// A type, which classifies regular variables (not type variables, not hypotheses).
pub type Ty = Rc<TyKind>;

/// A type, which classifies regular variables (not type variables, not hypotheses).
#[derive(Debug, DeepSizeOf)]
pub enum TyKind {
  /// `()` is the type with one element; `sizeof () = 0`.
  Unit,
  /// A true proposition.
  True,
  /// A false proposition.
  False,
  /// `bool` is the type of booleans, that is, bytes which are 0 or 1; `sizeof bool = 1`.
  Bool,
  /// A type variable.
  Var(TyVarId),
  /// The integral types:
  /// * `i(8*N)` is the type of N byte signed integers `sizeof i(8*N) = N`.
  /// * `u(8*N)` is the type of N byte unsigned integers; `sizeof u(8*N) = N`.
  Int(IntTy),
  /// The type `[T; n]` is an array of `n` elements of type `T`;
  /// `sizeof [T; n] = sizeof T * n`.
  Array(Ty, Expr),
  /// `own T` is a type of owned pointers. The typehood predicate is
  /// `x :> own T` iff `E. v (x |-> v) * v :> T`.
  Own(Ty),
  /// `(ref T)` is a type of borrowed values. This type is elaborated to
  /// `(ref a T)` where `a` is a lifetime; this is handled a bit differently than rust
  /// (see [`Lifetime`]).
  Ref(Lifetime, Ty),
  /// `&sn x` is the type of pointers to the place `x` (a variable or indexing expression).
  RefSn(Expr),
  /// `(sn {a : T})` the type of values of type `T` that are equal to `a`.
  /// This is useful for asserting that a computationally relevant value can be
  /// expressed in terms of computationally irrelevant parts.
  Sn(Expr, Ty),
  /// `{x : A, y : B, z : C}` is the dependent version of `list`;
  /// it is a tuple type with elements `A, B, C`, but the types `A, B, C` can
  /// themselves refer to `x, y, z`.
  /// `sizeof {x : A, _ : B x} = sizeof A + max_x (sizeof (B x))`.
  ///
  /// The top level declaration `(struct foo {x : A} {y : B})` desugars to
  /// `(typedef foo {x : A, y : B})`.
  Struct(Box<[Arg]>),
  /// A universally quantified proposition.
  All(VarId, Ty, Ty),
  /// Implication (plain, non-separating).
  Imp(Ty, Ty),
  /// Separating implication.
  Wand(Ty, Ty),
  /// Negation.
  Not(Ty),
  /// `(and A B C)` is an intersection type of `A, B, C`;
  /// `sizeof (and A B C) = max (sizeof A, sizeof B, sizeof C)`, and
  /// the typehood predicate is `x :> (and A B C)` iff
  /// `x :> A /\ x :> B /\ x :> C`. (Note that this is regular conjunction,
  /// not separating conjunction.)
  And(Box<[Ty]>),
  /// `(or A B C)` is an undiscriminated anonymous union of types `A, B, C`.
  /// `sizeof (or A B C) = max (sizeof A, sizeof B, sizeof C)`, and
  /// the typehood predicate is `x :> (or A B C)` iff
  /// `x :> A \/ x :> B \/ x :> C`.
  Or(Box<[Ty]>),
  /// `(or A B C)` is an undiscriminated anonymous union of types `A, B, C`.
  /// `sizeof (or A B C) = max (sizeof A, sizeof B, sizeof C)`, and
  /// the typehood predicate is `x :> (or A B C)` iff
  /// `x :> A \/ x :> B \/ x :> C`.
  If(Expr, Ty, Ty),
  /// `(ghost A)` is a computationally irrelevant version of `A`, which means
  /// that the logical storage of `(ghost A)` is the same as `A` but the physical storage
  /// is the same as `()`. `sizeof (ghost A) = 0`.
  Ghost(Ty),
  /// `(? T)` is the type of possibly-uninitialized `T`s. The typing predicate
  /// for this type is vacuous, but it has the same size as `T`, so overwriting with
  /// a `T` is possible.
  Uninit(Ty),
  /// A boolean expression, interpreted as a pure proposition
  Pure(Expr),
  /// A user-defined type-former.
  User(AtomId, Box<[Ty]>, Box<[Expr]>),
  /// A heap assertion `l |-> (v: |T|)`.
  Heap(Expr, Expr, Ty),
  /// An explicit typing assertion `[v : T]`.
  HasTy(Expr, Ty),
  /// The input token.
  Input,
  /// The output token.
  Output,
  /// A moved-away type.
  Moved(Ty),
}

impl Remap for TyKind {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self {
    match self {
      TyKind::Unit => TyKind::Unit,
      TyKind::True => TyKind::True,
      TyKind::False => TyKind::False,
      TyKind::Bool => TyKind::Bool,
      &TyKind::Var(v) => TyKind::Var(v),
      &TyKind::Int(ity) => TyKind::Int(ity),
      TyKind::Array(ty, n) => TyKind::Array(ty.remap(r), n.remap(r)),
      TyKind::Own(ty) => TyKind::Own(ty.remap(r)),
      TyKind::Ref(lft, ty) => TyKind::Ref(*lft, ty.remap(r)),
      TyKind::RefSn(e) => TyKind::RefSn(e.remap(r)),
      TyKind::Sn(a, ty) => TyKind::Sn(a.remap(r), ty.remap(r)),
      TyKind::Struct(args) => TyKind::Struct(args.remap(r)),
      TyKind::All(v, pat, ty) => TyKind::All(*v, pat.remap(r), ty.remap(r)),
      TyKind::Imp(p, q) => TyKind::Imp(p.remap(r), q.remap(r)),
      TyKind::Wand(p, q) => TyKind::Wand(p.remap(r), q.remap(r)),
      TyKind::Not(p) => TyKind::Not(p.remap(r)),
      TyKind::And(ps) => TyKind::And(ps.remap(r)),
      TyKind::Or(ps) => TyKind::Or(ps.remap(r)),
      TyKind::If(c, t, e) => TyKind::If(c.remap(r), t.remap(r), e.remap(r)),
      TyKind::Ghost(ty) => TyKind::Ghost(ty.remap(r)),
      TyKind::Uninit(ty) => TyKind::Uninit(ty.remap(r)),
      TyKind::Pure(e) => TyKind::Pure(e.remap(r)),
      TyKind::User(f, tys, es) => TyKind::User(*f, tys.remap(r), es.remap(r)),
      TyKind::Heap(e, v, ty) =>
        TyKind::Heap(e.remap(r), v.remap(r), ty.remap(r)),
      TyKind::HasTy(e, ty) => TyKind::HasTy(e.remap(r), ty.remap(r)),
      TyKind::Input => TyKind::Input,
      TyKind::Output => TyKind::Output,
      TyKind::Moved(ty) => TyKind::Moved(ty.remap(r)),
    }
  }
}

/// The type of variant, or well founded order that recursions decrease.
#[derive(Debug, DeepSizeOf)]
pub enum VariantType {
  /// This variant is a nonnegative natural number which decreases to 0.
  Down,
  /// This variant is a natural number or integer which increases while
  /// remaining less than this constant.
  UpLt(Expr),
  /// This variant is a natural number or integer which increases while
  /// remaining less than or equal to this constant.
  UpLe(Expr),
}

/// A variant is a pure expression, together with a
/// well founded order that decreases on all calls.
#[derive(Debug, DeepSizeOf)]
pub struct Variant(pub Expr, pub VariantType);

/// A pure expression. (Regular expressions are not manipulated like types,
/// i.e. copied and substituted around, so they are in the [`hir`](super::hir) module.)
pub type Expr = Rc<ExprKind>;

/// A pure expression. (Regular expressions are not manipulated like types,
/// i.e. copied and substituted around, so they are in the [`hir`](super::hir) module.)
pub type ExprTy = (Option<Expr>, Ty);

/// A pure expression.
#[derive(Debug, DeepSizeOf)]
pub enum ExprKind {
  /// A `()` literal.
  Unit,
  /// A variable reference.
  Var(VarId),
  /// A user constant.
  Const(AtomId),
  /// A number literal.
  Bool(bool),
  /// A number literal.
  Int(BigInt),
  /// A unary operation.
  Unop(Unop, Expr),
  /// A binary operation.
  Binop(Binop, Expr, Expr),
  /// An index operation `a[i]: T` where `a: (array T n)` and `i: nat`.
  Index(Expr, Expr),
  /// If `x: (array T n)`, then `x[a..a+b]: (array T b)`.
  Slice(Expr, Expr, Expr),
  /// A projection operation `x.i: T` where
  /// `x: (T0, ..., T(n-1))` or `x: {f0: T0, ..., f(n-1): T(n-1)}`.
  Proj(Expr, u32),
  /// `(update-index a i e)` is the result of `a` after `a[i] = e`.
  UpdateIndex(Expr, Expr, Expr),
  /// `(update-slice x a b e)` is the result of assigning `x[a..a+b] = e`.
  UpdateSlice(Expr, Expr, Expr, Expr),
  /// `(update-proj x i)` is the result of assigning `x.i = e`.
  UpdateProj(Expr, u32, Expr),
  /// `(e1, ..., en)` returns a tuple of the arguments.
  List(Box<[Expr]>),
  /// `[e1, ..., en]`, an array literal.
  Array(Box<[Expr]>),
  /// Return the size of a type.
  Sizeof(Ty),
  /// A pointer to a place.
  Ref(Expr),
  /// `(pure $e$)` embeds an MM0 expression `$e$` as the target type,
  /// one of the numeric types
  Mm0(Mm0Expr),
  /// A function call
  Call {
    /// The function to call.
    f: AtomId,
    /// The type arguments.
    tys: Box<[Ty]>,
    /// The function arguments.
    args: Box<[Expr]>,
  },
  /// An if-then-else expression (at either block or statement level). The initial atom names
  /// a hypothesis that the expression is true in one branch and false in the other.
  If {
    /// The if condition.
    cond: Expr,
    /// The then case.
    then: Expr,
    /// The else case.
    els: Expr
  },
}

impl Remap for ExprKind {
  type Target = Self;
  #[allow(clippy::many_single_char_names)]
  fn remap(&self, r: &mut Remapper) -> Self {
    match self {
      ExprKind::Unit => ExprKind::Unit,
      &ExprKind::Var(v) => ExprKind::Var(v),
      &ExprKind::Const(c) => ExprKind::Const(c),
      &ExprKind::Bool(b) => ExprKind::Bool(b),
      ExprKind::Int(n) => ExprKind::Int(n.clone()),
      ExprKind::Unop(op, e) => ExprKind::Unop(*op, e.remap(r)),
      ExprKind::Binop(op, e1, e2) => ExprKind::Binop(*op, e1.remap(r), e2.remap(r)),
      ExprKind::Index(a, i) => ExprKind::Index(a.remap(r), i.remap(r)),
      ExprKind::Slice(a, i, l) => ExprKind::Slice(a.remap(r), i.remap(r), l.remap(r)),
      ExprKind::Proj(a, i) => ExprKind::Proj(a.remap(r), *i),
      ExprKind::UpdateIndex(a, i, v) => ExprKind::UpdateIndex(a.remap(r), i.remap(r), v.remap(r)),
      ExprKind::UpdateSlice(a, i, l, v) =>
        ExprKind::UpdateSlice(a.remap(r), i.remap(r), l.remap(r), v.remap(r)),
      ExprKind::UpdateProj(a, i, v) => ExprKind::UpdateProj(a.remap(r), *i, v.remap(r)),
      ExprKind::List(es) => ExprKind::List(es.remap(r)),
      ExprKind::Array(es) => ExprKind::Array(es.remap(r)),
      ExprKind::Sizeof(ty) => ExprKind::Sizeof(ty.remap(r)),
      ExprKind::Ref(e) => ExprKind::Ref(e.remap(r)),
      ExprKind::Mm0(e) => ExprKind::Mm0(e.remap(r)),
      &ExprKind::Call {f, ref tys, ref args} =>
        ExprKind::Call {f, tys: tys.remap(r), args: args.remap(r)},
      ExprKind::If {cond, then, els} => ExprKind::If {
        cond: cond.remap(r), then: then.remap(r), els: els.remap(r)},
    }
  }
}

/// A basic block ID, which is used to look up blocks in the [`Cfg`].
#[derive(Copy, Clone, Default, Debug)]
pub struct BlockId(u32);
crate::deep_size_0!(BlockId);

impl BlockId {
  /// The ID of the entry block.
  pub const ENTRY: Self = Self(0);
}
impl Remap for BlockId {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self { *self }
}

/// A collection of contexts, maintaining a tree structure. The underlying data structure is a list
/// of `CtxBuf` structs, each of which is a `CtxId` pointer to another context, plus an additional
/// list of variables and types. The context at index 0 is the root context, and is its own parent.
#[derive(Debug, DeepSizeOf)]
pub struct Contexts(Vec<CtxBuf>);

impl Remap for Contexts {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self { Self(self.0.remap(r)) }
}

impl Index<CtxBufId> for Contexts {
  type Output = CtxBuf;
  fn index(&self, index: CtxBufId) -> &Self::Output { &self.0[index.0 as usize] }
}
impl IndexMut<CtxBufId> for Contexts {
  fn index_mut(&mut self, index: CtxBufId) -> &mut Self::Output { &mut self.0[index.0 as usize] }
}
impl Default for Contexts {
  fn default() -> Self { Self(vec![CtxBuf::default()]) }
}

impl Contexts {
  /// Given a context ID, retrieve a context buffer, ensuring that it can be directly extended by
  /// allocating a new context buffer if necessary.
  pub fn unshare(&mut self, id: &'_ mut CtxId) -> &mut CtxBuf {
    let mut ctx = &mut self[id.0];
    if u32::try_from(ctx.vars.len()).expect("overflow") == id.1 {
      /// Safety: NLL case 3 (polonius validates this borrow pattern)
      #[allow(clippy::useless_transmute)]
      unsafe { std::mem::transmute::<&mut CtxBuf, &mut CtxBuf>(ctx) }
    } else {
      let buf_id = CtxBufId(self.0.len().try_into().expect("overflow"));
      self.0.push(CtxBuf {parent: *id, vars: vec![]});
      *id = CtxId(buf_id, 1);
      unwrap_unchecked!(self.0.last_mut())
    }
  }

  /// Given a context, extend it with a variable and type to produce a new context.
  pub fn extend(&mut self, mut ctx: CtxId, var: VarId, ty: ExprTy) -> CtxId {
    self.unshare(&mut ctx).vars.push((var, ty));
    ctx
  }

  /// Returns an iterator over the variables and their values, in reverse order (from most
  /// recently added to least recent). This is more efficient than forward iteration, which must
  /// keep a stack.
  #[must_use] pub fn rev_iter(&self, CtxId(buf, i): CtxId) -> CtxIter<'_> {
    CtxIter {ctxs: self, buf, iter: self[buf].vars[..i as usize].iter()}
  }
}

/// The iterator struct returned by [`CtxIter::rev_iter`].
#[derive(Clone, Debug)]
pub struct CtxIter<'a> {
  ctxs: &'a Contexts,
  buf: CtxBufId,
  iter: std::slice::Iter<'a, (VarId, ExprTy)>,
}

impl<'a> Iterator for CtxIter<'a> {
  type Item = &'a (VarId, ExprTy);
  fn next(&mut self) -> Option<Self::Item> {
    loop {
      if let Some(v) = self.iter.next_back() {return Some(v)}
      if self.buf == CtxBufId::ROOT {return None}
      *self = self.ctxs.rev_iter(self.ctxs[self.buf].parent);
    }
  }
}


/// A CFG, or control flow graph, for a function. This consists of a set of basic blocks,
/// with block ID 0 being the entry block. The `ctxs` is the context data used to supply the
/// logical context at the beginning of each basic block.
#[derive(Default, Debug, DeepSizeOf)]
pub struct Cfg {
  /// The set of logical contexts for the basic blocks.
  pub ctxs: Contexts,
  /// The set of basic blocks, containing the actual code.
  pub blocks: Vec<BasicBlock>,
}

impl Remap for Cfg {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self {
    Self { ctxs: self.ctxs.remap(r), blocks: self.blocks.remap(r) }
  }
}

impl Cfg {
  /// Start a new basic block with the given initial context. This block starts unfinished, that
  /// is, with an empty `Terminator`; the terminator must be filled by the time MIR construction is
  /// complete.
  pub fn new_block(&mut self, parent: CtxId) -> BlockId {
    let id = BlockId(self.blocks.len().try_into().expect("block overflow"));
    self.blocks.push(BasicBlock::new(parent, None));
    id
  }
}

impl Index<CtxBufId> for Cfg {
  type Output = CtxBuf;
  fn index(&self, index: CtxBufId) -> &CtxBuf { &self.ctxs[index] }
}
impl IndexMut<CtxBufId> for Cfg {
  fn index_mut(&mut self, index: CtxBufId) -> &mut CtxBuf { &mut self.ctxs[index] }
}
impl Index<BlockId> for Cfg {
  type Output = BasicBlock;
  fn index(&self, index: BlockId) -> &BasicBlock { &self.blocks[index.0 as usize] }
}
impl IndexMut<BlockId> for Cfg {
  fn index_mut(&mut self, index: BlockId) -> &mut BasicBlock { &mut self.blocks[index.0 as usize] }
}

/// A "context buffer ID", which points to one of the context buffers in the [`Contexts`] struct.
#[derive(Copy, Clone, Debug, Default, DeepSizeOf, PartialEq, Eq)]
pub struct CtxBufId(u32);

impl CtxBufId {
  /// The root context buffer is the first one; this is its own parent.
  pub const ROOT: Self = Self(0);
}

/// A context ID, which consists of a context buffer ID (which selects a context buffer from the
/// [`Contexts`]), plus an index into that buffer. The logical context denoted includes all
/// contexts in the parent chain up to the root, plus the selected context buffer up to the
/// specified index (which may be any number `<= buf.len()`).
#[derive(Copy, Clone, Debug, Default, DeepSizeOf)]
pub struct CtxId(CtxBufId, u32);

impl CtxId {
  /// The empty context.
  pub const ROOT: Self = Self(CtxBufId::ROOT, 0);
}

/// A context buffer.
#[derive(Default, Debug, DeepSizeOf)]
pub struct CtxBuf {
  /// The parent context, which this buffer is viewed as extending.
  pub parent: CtxId,
  /// The additional variables that this buffer adds to the context.
  pub vars: Vec<(VarId, ExprTy)>,
}

impl Remap for CtxBuf {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self {
    Self { parent: self.parent, vars: self.vars.remap(r) }
  }
}

/// The different kinds of projection, used in defining places.
#[derive(Copy, Clone, Debug)]
pub enum ProjectionKind {
  /// A projection `a.i` which retrieves the `i`th element of a tuple.
  Struct,
  /// A projection `a[i]` which retrieves the `i`th element of an array.
  Array,
  /// A projection `a.i` which views a conjunction type as its `i`th conjunct.
  And,
  /// A projection `a.0` which views a value `a: (sn {x : T})` type as `a.0: T`.
  Sn
}
crate::deep_size_0!(ProjectionKind);

impl From<hir::ProjectionKind> for ProjectionKind {
  fn from(pk: hir::ProjectionKind) -> Self {
    match pk {
      hir::ProjectionKind::List |
      hir::ProjectionKind::Struct => Self::Struct,
      hir::ProjectionKind::And => Self::And,
    }
  }
}

/// A place is a sequence of projections on a local. A projection is an array index or slice,
/// dereference, or a tuple projection.
#[derive(Copy, Clone, Debug)]
pub enum Projection {
  /// A constant projection into a tuple, array, or and. These projections are generated by tuple
  /// patterns.
  Proj(ProjectionKind, u32),
  /// A variable index into an array. `(index _ i h)`, where `h: i < n` and `_` has type
  /// `(array T n)`.
  Index(VarId, VarId),
  /// A variable slice into an array. `(slice _ i l h)`, where `h: i + l <= n` and `_` has type
  /// `(array T n)`.
  Slice(VarId, VarId, VarId),
  /// A dereference operation `(* _)` on a pointer.
  Deref,
}
crate::deep_size_0!(Projection);

/// A place is a location in memory that can be read and written to.
#[derive(Clone, Debug, DeepSizeOf)]
pub struct Place {
  /// A local variable as the source of the place.
  pub local: VarId,
  /// A list of projections on the variable to extract the relevant subpart.
  pub proj: Vec<Projection>,
}
impl Place {
  /// Construct a place directly from a local.
  #[must_use] pub fn local(local: VarId) -> Self { Self {local, proj: vec![]} }
}

impl From<VarId> for Place {
  fn from(v: VarId) -> Place { Place::local(v) }
}

impl Remap for Place {
  type Target = Self;
  #[inline] fn remap(&self, _: &mut Remapper) -> Self { self.clone() }
}

/// A constant value.
#[derive(Clone, Debug, DeepSizeOf)]
pub struct Constant {
  /// The type and value of the constant.
  pub ety: ExprTy,
  /// The value of the constant.
  pub k: ConstKind,
}

impl Constant {
  /// Returns a unit constant.
  #[must_use] pub fn unit() -> Self {
    Self { ety: (Some(Rc::new(ExprKind::Unit)), Rc::new(TyKind::Unit)), k: ConstKind::Unit }
  }

  /// Returns a true constant.
  #[must_use] pub fn itrue() -> Self {
    Self { ety: (Some(Rc::new(ExprKind::Unit)), Rc::new(TyKind::True)), k: ConstKind::ITrue }
  }

  /// Returns an uninit constant of the specified type.
  #[must_use] pub fn uninit(ty: Ty) -> Self {
    Self { ety: (Some(Rc::new(ExprKind::Unit)), Rc::new(TyKind::Uninit(ty))), k: ConstKind::Uninit }
  }

  /// Returns a boolean constant.
  #[must_use] pub fn bool(b: bool) -> Self {
    Self { ety: (Some(Rc::new(ExprKind::Bool(b))), Rc::new(TyKind::Bool)), k: ConstKind::Bool }
  }

  /// Returns an integral constant.
  #[must_use] pub fn int(ty: IntTy, n: BigInt) -> Self {
    Self { ety: (Some(Rc::new(ExprKind::Int(n))), Rc::new(TyKind::Int(ty))), k: ConstKind::Int }
  }
}

impl Remap for Constant {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self {
    Self { ety: self.ety.remap(r), k: self.k.remap(r) }
  }
}

/// The different types of constant.
#[derive(Copy, Clone, Debug, DeepSizeOf)]
pub enum ConstKind {
  /// A unit constant `()`.
  Unit,
  /// A true constant `()`.
  ITrue,
  /// A boolean constant.
  Bool,
  /// An integer constant.
  Int,
  /// The constant `uninit`, which has type `(? T)`. Used as an rvalue,
  /// this means the target place can receive any bit pattern.
  Uninit,
  /// A named constant.
  Const(AtomId),
}

impl Remap for ConstKind {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self {
    match *self {
      Self::Unit => Self::Unit,
      Self::ITrue => Self::ITrue,
      Self::Bool => Self::Bool,
      Self::Int => Self::Int,
      Self::Uninit => Self::Uninit,
      Self::Const(a) => Self::Const(a.remap(r)),
    }
  }
}

/// An rvalue is an expression that can be used as the right hand side of an assignment;
/// most side-effect-free expressions fall in this category.
#[derive(Clone, Debug, DeepSizeOf)]
pub enum Operand {
  /// Copies the value at the given place. Requires that the type of the place is a copy type.
  Copy(Place),
  /// Moves the value out of the given place, which must not be used again.
  Move(Place),
  /// Copies the moved version of the data at the given place.
  Ref(Place),
  /// Synthesize a constant value.
  Const(Box<Constant>),
}

impl Remap for Operand {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self {
    match self {
      Operand::Copy(x) => Operand::Copy(x.remap(r)),
      Operand::Move(x) => Operand::Move(x.remap(r)),
      Operand::Ref(x) => Operand::Ref(x.remap(r)),
      Operand::Const(x) => Operand::Const(x.remap(r)),
    }
  }
}

impl From<Constant> for Operand {
  #[inline] fn from(c: Constant) -> Operand { Operand::Const(Box::new(c)) }
}
impl From<Place> for Operand {
  #[inline] fn from(p: Place) -> Operand { Operand::Move(p) }
}
impl From<VarId> for Operand {
  #[inline] fn from(v: VarId) -> Operand { Place::local(v).into() }
}

impl Operand {
  /// Convert an operand to an rvalue.
  #[inline] #[must_use] pub fn rv(self) -> RValue { RValue::Use(self) }
}

/// A proof that `x: T` can be retyped as `U`.
#[derive(Clone, Debug, DeepSizeOf)]
pub enum CastKind {
  /// * `Cast(x, Sn(None))` proves that `x: sn x`
  /// * `Cast(x, Sn(Some(h)))` proves that `x: sn y` where `h: x = y`
  Sn(Option<Operand>),
}

impl Remap for CastKind {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self {
    match self {
      CastKind::Sn(h) => CastKind::Sn(h.remap(r))
    }
  }
}

/// An rvalue is an expression that can be used as the right hand side of an assignment;
/// most side-effect-free expressions fall in this category.
#[derive(Clone, Debug, DeepSizeOf)]
pub enum RValue {
  /// Directly use a place value or constant.
  Use(Operand),
  /// Apply a unary operator.
  Unop(Unop, Operand),
  /// Apply a binary operator.
  Binop(Binop, Operand, Operand),
  /// Construct an lvalue reference with the specified type.
  Cast(Place, CastKind),
  /// Move the target place into a ghost variable.
  Ghost(Operand),
}

impl Remap for RValue {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self {
    match self {
      RValue::Use(e) => RValue::Use(e.remap(r)),
      RValue::Unop(op, e) => RValue::Unop(*op, e.remap(r)),
      RValue::Binop(op, e1, e2) => RValue::Binop(*op, e1.remap(r), e2.remap(r)),
      RValue::Cast(e, ck) => RValue::Cast(e.remap(r), ck.remap(r)),
      RValue::Ghost(e) => RValue::Ghost(e.remap(r)),
    }
  }
}

impl From<Operand> for RValue {
  #[inline] fn from(op: Operand) -> RValue { op.rv() }
}
impl From<Constant> for RValue {
  #[inline] fn from(c: Constant) -> RValue { Operand::from(c).rv() }
}
impl From<Place> for RValue {
  #[inline] fn from(p: Place) -> RValue { Operand::from(p).rv() }
}
impl From<VarId> for RValue {
  #[inline] fn from(v: VarId) -> RValue { Place::local(v).into() }
}

/// The different kinds of existential elimination statement.
#[derive(Clone, Debug, DeepSizeOf)]
pub enum ExElimKind {
  /// `Own(x, T, p, &sn x)` is an existential pattern match on `(own T)`, producing a
  /// value `x` and a pointer `p: &sn x`.
  Own([(VarId, Ty); 2]),
}

impl Remap for ExElimKind {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self {
    match self {
      Self::Own([(x, xt), (y, yt)]) => Self::Own([(*x, xt.remap(r)), (*y, yt.remap(r))])
    }
  }
}

/// A statement is an operation in a basic block that does not end the block. Generally this means
/// that it has simple control flow behavior, in that it always steps to the following statement
/// after performing some action that cannot fail.
#[derive(Clone, Debug, DeepSizeOf)]
pub enum Statement {
  /// A declaration of a variable with a value, `let x: T = rv;`
  Let(VarId, ExprTy, RValue),
  /// An exists destructuring, `let (x, h): (exists x: T, P x) = rv;`
  ExElim(ExElimKind, Ty, RValue),
}

impl Remap for Statement {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self {
    match self {
      Self::Let(x, ty, rv) => Self::Let(*x, ty.remap(r), rv.remap(r)),
      Self::ExElim(ek, ty, rv) => Self::ExElim(ek.remap(r), ty.remap(r), rv.remap(r)),
    }
  }
}

/// A terminator is the final statement in a basic block. Anything with nontrivial control flow
/// is a terminator, and it determines where to jump afterward.
#[derive(Clone, Debug, DeepSizeOf)]
pub enum Terminator {
  /// A `goto label(x -> arg,*);` statement - unconditionally jump to the basic block `label`.
  /// The `x -> arg` values assign values to variables, where `x` is a variable in the context of
  /// the target and `arg` is an operand evaluated in the current basic block context.
  /// Any variables `x` that do not exist in the target context are ignored, and variables in the
  /// intersection of the two contexts are optional, where if they are not specified then they
  /// are assumed to keep their values. Variables in the target context but not the source must
  /// be specified.
  Jump(BlockId, Vec<(VarId, Operand)>),
  /// A `return(x -> arg,*);` statement - unconditionally return from the function.
  /// The `x -> arg` values assign values to variables, where `x` is a variable in the function
  /// returns and `arg` is an operand evaluated in the current basic block context.
  Return(Vec<(VarId, Operand)>),
  /// A `unreachable e;` statement takes a proof `e` of false and cancels this basic block.
  /// Later optimization passes will attempt to delete the entire block.
  Unreachable(Operand),
  /// A branch expression `if cond {h. goto l1} else {h'. goto l2}`.
  /// We require that context of `l1` extends the current one with `h: cond`,
  /// and the context of `l2` extends the current one with `h': !cond`.
  If(Operand, [(VarId, BlockId); 2]),
  /// An assert expression `if cond {h. goto l1} else {fail}`.
  /// This is lowered the same as a branch, but there is no actual `fail` basic block to
  /// jump to.
  Assert(Operand, VarId, BlockId),
}

impl Remap for Terminator {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self {
    match self {
      Self::Jump(id, args) => Self::Jump(*id, args.remap(r)),
      Self::Return(args) => Self::Return(args.remap(r)),
      Self::Unreachable(rv) => Self::Unreachable(rv.remap(r)),
      Self::If(cond, args) => Self::If(cond.remap(r), *args),
      Self::Assert(cond, v, bl) => Self::Assert(cond.remap(r), *v, *bl),
    }
  }
}

/// A basic block, which consists of an initial context (containing the logical parameters to the
/// block), followed by a list of statements, and ending with a terminator. The terminator is
/// optional only during MIR construction, and represents an "unfinished" block.
#[derive(Debug, DeepSizeOf)]
pub struct BasicBlock {
  /// The initial context on entry to the block.
  pub ctx: CtxId,
  /// The list of statements, which may extend the context.
  pub stmts: Vec<Statement>,
  /// The final statement, which may jump to another basic block or perform another control flow
  /// function.
  pub term: Option<Terminator>,
}

impl Remap for BasicBlock {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self {
    Self { ctx: self.ctx, stmts: self.stmts.remap(r), term: self.term.remap(r) }
  }
}

impl BasicBlock {
  fn new(ctx: CtxId, term: Option<Terminator>) -> Self {
    Self { ctx, stmts: vec![], term }
  }

  /// Finish this basic block by adding the terminator.
  /// It is a bug to terminate a basic block that is already terminated.
  pub fn terminate(&mut self, term: Terminator) {
    assert!(mem::replace(&mut self.term, Some(term)).is_none())
  }
}

/// A procedure (or function or intrinsic), a top level item similar to function declarations in C.
#[derive(Debug, DeepSizeOf)]
pub struct Proc {
  /// The type of declaration: `func`, `proc`, or `intrinsic`.
  kind: ProcKind,
  /// The name of the procedure.
  name: Spanned<AtomId>,
  /// The number of type arguments
  tyargs: u32,
  /// The arguments of the procedure.
  args: Vec<Arg>,
  /// The return values of the procedure. (Functions and procedures return multiple values in MMC.)
  rets: Vec<Arg>,
  /// The body of the procedure.
  body: Cfg,
}

impl Remap for Proc {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self {
    Self {
      kind: self.kind,
      name: self.name.remap(r),
      tyargs: self.tyargs,
      args: self.args.remap(r),
      rets: self.rets.remap(r),
      body: self.body.remap(r),
    }
  }
}
