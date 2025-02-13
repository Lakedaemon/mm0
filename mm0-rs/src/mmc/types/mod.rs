//! Types used in the stages of the compiler.

pub mod parse;
pub mod ast;
pub mod entity;
pub mod ty;
pub mod global;
pub mod hir;
pub mod mir;
pub mod pir;

use std::{borrow::Cow, collections::HashMap, convert::{TryFrom, TryInto}, rc::Rc};
use num::{BigInt, Signed};

use crate::{AtomId, Environment, Remap, Remapper, TermId, LispVal, lisp::Syntax,
  EnvDisplay, FormatEnv, FileSpan};

/// A variable ID. These are local to a given declaration (function, constant, global),
/// but are not de Bruijn variables - they are unique identifiers within the declaration.
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

/// A spanned expression.
#[derive(Clone, Debug, DeepSizeOf)]
pub struct Spanned<T> {
  /// The span of the expression
  pub span: FileSpan,
  /// The data (the `k` stands for `kind` because it's often a `*Kind` enum
  /// but it can be anything).
  pub k: T,
}

impl<T> Spanned<T> {
  /// Transform a `Spanned<T>` into `Spanned<U>` given `f: T -> U`.
  pub fn map_into<U>(self, f: impl FnOnce(T) -> U) -> Spanned<U> {
    Spanned { span: self.span, k: f(self.k) }
  }
}

impl<T: Remap> Remap for Spanned<T> {
  type Target = Spanned<T::Target>;
  fn remap(&self, r: &mut Remapper) -> Spanned<T::Target> {
    Spanned {span: self.span.clone(), k: self.k.remap(r)}
  }
}

macro_rules! make_keywords {
  {$($(#[$attr:meta])* $x:ident: $e:expr,)*} => {
    make_keywords! {@IMPL $($(#[$attr])* $x concat!("The keyword `", $e, "`.\n"), $e,)*}
  };
  {@IMPL $($(#[$attr:meta])* $x:ident $doc0:expr, $e:expr,)*} => {
    /// The type of MMC keywords, which are atoms with a special role in the MMC parser.
    #[derive(Debug, EnvDebug, PartialEq, Eq, Copy, Clone)]
    pub enum Keyword { $(#[doc=$doc0] $(#[$attr])* $x),* }
    crate::deep_size_0!(Keyword);

    lazy_static! {
      static ref SYNTAX_MAP: Box<[Option<Keyword>]> = {
        let mut vec = vec![];
        Syntax::for_each(|_, name| vec.push(Keyword::from_str(name)));
        vec.into()
      };
    }

    impl Keyword {
      #[must_use] fn from_str(s: &str) -> Option<Self> {
        match s {
          $($e => Some(Self::$x),)*
          _ => None
        }
      }

      /// Get the MMC keyword corresponding to a lisp [`Syntax`].
      #[must_use] pub fn from_syntax(s: Syntax) -> Option<Self> {
        SYNTAX_MAP[s as usize]
      }
    }

    impl Environment {
      /// Make the initial MMC keyword map in the given environment.
      #[allow(clippy::string_lit_as_bytes)]
      pub fn make_keywords(&mut self) -> HashMap<AtomId, Keyword> {
        let mut atoms = HashMap::new();
        $(if Syntax::from_str($e).is_none() {
          atoms.insert(self.get_atom($e.as_bytes()), Keyword::$x);
        })*
        atoms
      }
    }
  }
}

make_keywords! {
  Add: "+",
  Arrow: "=>",
  ArrowL: "<-",
  ArrowR: "->",
  Begin: "begin",
  Colon: ":",
  ColonEq: ":=",
  Const: "const",
  Else: "else",
  Entail: "entail",
  Func: "func",
  Finish: "finish",
  Ghost: "ghost",
  Global: "global",
  Implicit: "implicit",
  Intrinsic: "intrinsic",
  If: "if",
  Le: "<=",
  Lt: "<",
  Match: "match",
  Mut: "mut",
  Or: "or",
  Out: "out",
  Proc: "proc",
  Star: "*",
  Struct: "struct",
  Typedef: "typedef",
  Variant: "variant",
  While: "while",
  With: "with",
}

/// Possible sizes for integer operations and types.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Size {
  /// 8 bits, or 1 byte. Used for `u8` and `i8`.
  S8,
  /// 16 bits, or 2 bytes. Used for `u16` and `i16`.
  S16,
  /// 32 bits, or 4 bytes. Used for `u32` and `i32`.
  S32,
  /// 64 bits, or 8 bytes. Used for `u64` and `i64`.
  S64,
  /// Unbounded size. Used for `nat` and `int`. (These types are only legal for
  /// ghost variables, but they are also used to indicate "correct to an unbounded model"
  /// for operations like [`Unop::BitNot`] when it makes sense. We do not actually support
  /// bignum compilation.)
  Inf,
}
crate::deep_size_0!(Size);

impl Default for Size {
  fn default() -> Self { Self::Inf }
}

impl Size {
  /// The number of bits of this type, or `None` for the infinite case.
  #[must_use] pub fn bits(self) -> Option<u8> {
    match self {
      Size::Inf => None,
      Size::S8 => Some(8),
      Size::S16 => Some(16),
      Size::S32 => Some(32),
      Size::S64 => Some(64),
    }
  }

  /// The number of bytes of this type, or `None` for the infinite case.
  #[must_use] pub fn bytes(self) -> Option<u8> {
    match self {
      Size::Inf => None,
      Size::S8 => Some(1),
      Size::S16 => Some(2),
      Size::S32 => Some(4),
      Size::S64 => Some(8),
    }
  }
}

/// The set of integral types, `N_s` and `Z_s`, representing the signed and unsigned integers
/// of various bit widths, plus the computationally unrepresentable types of
/// unbounded natural numbers and unbounded integers.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum IntTy {
  /// The type of signed integers of given bit width, or all integers.
  Int(Size),
  /// The type of unsigned integers of given bit width, or all nonnegative integers.
  UInt(Size),
}
crate::deep_size_0!(IntTy);

impl std::fmt::Display for IntTy {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    self.to_str().fmt(f)
  }
}

impl IntTy {
  /// The size of this integral type.
  #[must_use] pub fn size(self) -> Size {
    match self { IntTy::Int(sz) | IntTy::UInt(sz) => sz }
  }

  /// A string description of this type.
  #[must_use] pub fn to_str(self) -> &'static str {
    match self {
      IntTy::Int(Size::Inf) => "int",
      IntTy::Int(Size::S8) => "i8",
      IntTy::Int(Size::S16) => "i16",
      IntTy::Int(Size::S32) => "i32",
      IntTy::Int(Size::S64) => "i64",
      IntTy::UInt(Size::Inf) => "nat",
      IntTy::UInt(Size::S8) => "u8",
      IntTy::UInt(Size::S16) => "u16",
      IntTy::UInt(Size::S32) => "u32",
      IntTy::UInt(Size::S64) => "u64",
    }
  }

  /// Returns true if `n` is a valid member of this integral type.
  #[must_use] pub fn contains(self, n: &BigInt) -> bool {
    match self {
      IntTy::Int(Size::Inf) => true,
      IntTy::Int(Size::S8) => i8::try_from(n).is_ok(),
      IntTy::Int(Size::S16) => i16::try_from(n).is_ok(),
      IntTy::Int(Size::S32) => i32::try_from(n).is_ok(),
      IntTy::Int(Size::S64) => i64::try_from(n).is_ok(),
      IntTy::UInt(Size::Inf) => !n.is_negative(),
      IntTy::UInt(Size::S8) => u8::try_from(n).is_ok(),
      IntTy::UInt(Size::S16) => u16::try_from(n).is_ok(),
      IntTy::UInt(Size::S32) => u32::try_from(n).is_ok(),
      IntTy::UInt(Size::S64) => u64::try_from(n).is_ok(),
    }
  }
}

impl PartialOrd for IntTy {
  /// `IntTy` is partially ordered by inclusion.
  fn le(&self, other: &Self) -> bool {
    match (self, other) {
      (IntTy::Int(sz1), IntTy::Int(sz2)) |
      (IntTy::UInt(sz1), IntTy::UInt(sz2)) => sz1 <= sz2,
      (IntTy::Int(_), IntTy::UInt(_)) => false,
      (IntTy::UInt(sz1), IntTy::Int(sz2)) => sz1 < sz2,
    }
  }

  fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
    match (self <= other, other <= self) {
      (true, true) => Some(std::cmp::Ordering::Equal),
      (true, false) => Some(std::cmp::Ordering::Less),
      (false, true) => Some(std::cmp::Ordering::Greater),
      (false, false) => None,
    }
  }
  fn lt(&self, other: &Self) -> bool { self <= other && self != other }
  fn gt(&self, other: &Self) -> bool { other < self }
  fn ge(&self, other: &Self) -> bool { other <= self }
}

/// (Elaborated) unary operations.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Unop {
  /// Integer negation
  Neg,
  /// Logical (boolean) NOT
  Not,
  /// Bitwise NOT. For fixed size this is the operation `2^n - x - 1`, and
  /// for infinite size this is `-x - 1`. Note that signed NOT always uses
  /// [`Size::Inf`].
  ///
  /// Infinite size is also the default value before type checking.
  BitNot(Size),
  /// Truncation into the given type. For fixed size this is the operation `x % 2^n`,
  /// for `int` this is the identity, and for `nat` this is invalid.
  As(IntTy),
}
crate::deep_size_0!(Unop);

impl Unop {
  /// Return a string representation of the [`Unop`].
  #[must_use] pub fn to_str(self) -> &'static str {
    match self {
      Unop::Neg => "-",
      Unop::Not => "not",
      Unop::BitNot(_) => "bnot",
      Unop::As(_) => "as..",
    }
  }

  /// Returns true if this takes integral arguments,
  /// and false if it takes booleans.
  #[must_use] pub fn int_in_out(self) -> bool {
    match self {
      Unop::Neg |
      Unop::BitNot(_) |
      Unop::As(_) => true,
      Unop::Not => false,
    }
  }

  /// Apply this unary operation as a `bool -> bool` function.
  /// Panics if it is not a `bool -> bool` function.
  #[must_use] pub fn apply_bool(self, b: bool) -> bool {
    match self {
      Unop::Not => !b,
      Unop::Neg |
      Unop::BitNot(_) |
      Unop::As(_) => panic!("not a bool op"),
    }
  }

  /// Apply this unary operation as a `int -> int` function. Returns `None` if the function
  /// inputs are out of range or if it is not a `int -> int` function.
  #[must_use] pub fn apply_int(self, n: &BigInt) -> Option<Cow<'_, BigInt>> {
    macro_rules! truncate_signed {($iN:ty, $uN:ty) => {{
      if <$iN>::try_from(n).is_ok() { Cow::Borrowed(n) }
      else { Cow::Owned((<$uN>::try_from(n & BigInt::from(<$uN>::MAX)).unwrap() as $iN).into()) }
    }}}
    macro_rules! truncate_unsigned {($uN:ty) => {{
      if <$uN>::try_from(n).is_ok() { Cow::Borrowed(n) }
      else { Cow::Owned(n & BigInt::from(<$uN>::MAX)) }
    }}}
    match self {
      Unop::Neg => Some(Cow::Owned(-n)),
      Unop::Not => None,
      Unop::BitNot(Size::Inf) => Some(Cow::Owned(!n)),
      Unop::BitNot(Size::S8) => Some(Cow::Owned(u8::into(!n.try_into().ok()?))),
      Unop::BitNot(Size::S16) => Some(Cow::Owned(u16::into(!n.try_into().ok()?))),
      Unop::BitNot(Size::S32) => Some(Cow::Owned(u32::into(!n.try_into().ok()?))),
      Unop::BitNot(Size::S64) => Some(Cow::Owned(u64::into(!n.try_into().ok()?))),
      Unop::As(IntTy::Int(Size::Inf)) => Some(Cow::Borrowed(n)),
      Unop::As(IntTy::Int(Size::S8)) => Some(truncate_signed!(i8, u8)),
      Unop::As(IntTy::Int(Size::S16)) => Some(truncate_signed!(i16, u16)),
      Unop::As(IntTy::Int(Size::S32)) => Some(truncate_signed!(i32, u32)),
      Unop::As(IntTy::Int(Size::S64)) => Some(truncate_signed!(i64, u64)),
      Unop::As(IntTy::UInt(Size::Inf)) => panic!("{}", "{n as nat} does not exist"),
      Unop::As(IntTy::UInt(Size::S8)) => Some(truncate_unsigned!(u8)),
      Unop::As(IntTy::UInt(Size::S16)) => Some(truncate_unsigned!(u16)),
      Unop::As(IntTy::UInt(Size::S32)) => Some(truncate_unsigned!(u32)),
      Unop::As(IntTy::UInt(Size::S64)) => Some(truncate_unsigned!(u64)),
    }
  }
}

impl std::fmt::Display for Unop {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    self.to_str().fmt(f)
  }
}

/// Classification of the binary operations into types.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BinopType {
  /// `(int, int) -> int` functions, like `x + y`, `x * y`, `x & y`
  IntIntInt,
  /// `(int, int) -> bool` functions, like `x < y`, `x = y`, `x <= y`
  IntIntBool,
  /// `(int, nat) -> int` functions: `x << y` and `x >> y`
  IntNatInt,
  /// `(bool, bool) -> bool` functions: `x && y` and `x || y`
  BoolBoolBool,
}

impl BinopType {
  /// Does this function take integral types as input, or booleans?
  #[must_use] pub fn int_in(self) -> bool {
    matches!(self, Self::IntIntInt | Self::IntIntBool | Self::IntNatInt)
  }
  /// Does this function produce integral types as output, or booleans?
  #[must_use] pub fn int_out(self) -> bool {
    matches!(self, Self::IntIntInt | Self::IntNatInt)
  }
}

/// (Elaborated) binary operations.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Binop {
  /// Integer addition
  Add,
  /// Integer multiplication
  Mul,
  /// Integer subtraction
  Sub,
  /// Maximum
  Max,
  /// Minimum
  Min,
  /// Logical (boolean) AND
  And,
  /// Logical (boolean) OR
  Or,
  /// Bitwise AND, for signed or unsigned integers of any size
  BitAnd,
  /// Bitwise OR, for signed or unsigned integers of any size
  BitOr,
  /// Bitwise XOR, for signed or unsigned integers of any size
  BitXor,
  /// Shift left
  Shl,
  /// Shift right (arithmetic)
  Shr,
  /// Less than, for signed or unsigned integers of any size
  Lt,
  /// Less than or equal, for signed or unsigned integers of any size
  Le,
  /// Equal, for signed or unsigned integers of any size
  Eq,
  /// Not equal, for signed or unsigned integers of any size
  Ne,
}
crate::deep_size_0!(Binop);

impl Binop {
  /// Return a string representation of the [`Binop`].
  #[must_use] pub fn to_str(self) -> &'static str {
    match self {
      Binop::Add => "+",
      Binop::Mul => "*",
      Binop::Sub => "-",
      Binop::Max => "max",
      Binop::Min => "min",
      Binop::And => "and",
      Binop::Or => "or",
      Binop::BitAnd => "band",
      Binop::BitOr => "bor",
      Binop::BitXor => "bxor",
      Binop::Shl => "shl",
      Binop::Shr => "shr",
      Binop::Lt => "<",
      Binop::Le => "<=",
      Binop::Eq => "=",
      Binop::Ne => "!=",
    }
  }

  /// Returns the type of this binop.
  #[must_use] pub fn ty(self) -> BinopType {
    match self {
      Binop::Add | Binop::Mul | Binop::Sub |
      Binop::Max | Binop::Min |
      Binop::BitAnd | Binop::BitOr | Binop::BitXor => BinopType::IntIntInt,
      Binop::Shl | Binop::Shr => BinopType::IntNatInt,
      Binop::Lt | Binop::Le | Binop::Eq | Binop::Ne => BinopType::IntIntBool,
      Binop::And | Binop::Or => BinopType::BoolBoolBool,
    }
  }

  /// Returns true if this integral function returns a `nat` on nonnegative inputs.
  #[must_use] pub fn preserves_nat(self) -> bool {
    match self {
      Binop::Add | Binop::Mul |
      Binop::Max | Binop::Min |
      Binop::BitAnd | Binop::BitOr | Binop::BitXor |
      Binop::Shl | Binop::Shr => true,
      Binop::Sub => false,
      Binop::Lt | Binop::Le | Binop::Eq | Binop::Ne |
      Binop::And | Binop::Or => panic!("not an int -> int binop"),
    }
  }

  /// Returns true if this integral function returns a `UInt(sz)` on `UInt(sz)` inputs.
  #[must_use] pub fn preserves_usize(self) -> bool {
    match self {
      Binop::Add | Binop::Mul |
      Binop::Max | Binop::Min |
      Binop::Shl | Binop::Sub => false,
      Binop::BitAnd | Binop::BitOr | Binop::BitXor | Binop::Shr => true,
      Binop::Lt | Binop::Le | Binop::Eq | Binop::Ne |
      Binop::And | Binop::Or => panic!("not an int -> int binop"),
    }
  }

  /// Apply this unary operation as a `(int, int) -> int` function. Returns `None` if the function
  /// inputs are out of range or if it is not a `(int, int) -> int` function.
  /// (The `(int, nat) -> int` functions are also evaluated here.)
  #[must_use] pub fn apply_int_int(self, n1: &BigInt, n2: &BigInt) -> Option<BigInt> {
    match self {
      Binop::Add => Some(n1 + n2),
      Binop::Mul => Some(n1 * n2),
      Binop::Sub => Some(n1 - n2),
      Binop::Max => Some(n1.max(n2).clone()),
      Binop::Min => Some(n1.min(n2).clone()),
      Binop::BitAnd => Some(n1 & n2),
      Binop::BitOr => Some(n1 | n2),
      Binop::BitXor => Some(n1 ^ n2),
      Binop::Shl => Some(n1 << usize::try_from(n2).ok()?),
      Binop::Shr => Some(n1 >> usize::try_from(n2).ok()?),
      Binop::Lt | Binop::Le | Binop::Eq | Binop::Ne |
      Binop::And | Binop::Or => None,
    }
  }

  /// Apply this unary operation as a `(int, int) -> bool` function.
  /// Panics if it is not a `(int, int) -> bool` function.
  #[must_use] pub fn apply_int_bool(self, n1: &BigInt, n2: &BigInt) -> bool {
    match self {
      Binop::Lt => n1 < n2,
      Binop::Le => n1 <= n2,
      Binop::Eq => n1 == n2,
      Binop::Ne => n1 != n2,
      Binop::Add | Binop::Mul | Binop::Sub |
      Binop::Max | Binop::Min |
      Binop::BitAnd | Binop::BitOr | Binop::BitXor |
      Binop::Shl | Binop::Shr |
      Binop::And | Binop::Or => panic!("not int -> int -> bool binop"),
    }
  }

  /// Apply this unary operation as a `(bool, bool) -> bool` function.
  /// Panics if it is not a `(bool, bool) -> bool` function.
  #[must_use] pub fn apply_bool_bool(self, b1: bool, b2: bool) -> bool {
    match self {
      Binop::Add | Binop::Mul | Binop::Sub |
      Binop::Max | Binop::Min |
      Binop::BitAnd | Binop::BitOr | Binop::BitXor |
      Binop::Shl | Binop::Shr |
      Binop::Lt | Binop::Le | Binop::Eq | Binop::Ne => panic!("not bool -> bool -> bool binop"),
      Binop::And => b1 && b2,
      Binop::Or => b1 || b2,
    }
  }
}

impl std::fmt::Display for Binop {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    self.to_str().fmt(f)
  }
}

/// A field accessor.
#[derive(Copy, Clone, Debug)]
pub enum FieldName {
  /// A numbered field access like `x.1`.
  Number(u32),
  /// A named field access like `x.foo`.
  Named(AtomId),
}
crate::deep_size_0!(FieldName);

impl EnvDisplay for FieldName {
  fn fmt(&self, fe: FormatEnv<'_>, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    use std::fmt::Display;
    match *self {
      FieldName::Number(n) => n.fmt(f),
      FieldName::Named(a) => a.fmt(fe, f),
    }
  }
}

/// An embedded MM0 expression inside MMC. This representation is designed to make it easy
/// to produce substitutions of the free variables.
#[derive(Clone, Debug, DeepSizeOf)]
pub enum Mm0ExprNode {
  /// A constant expression, containing no free variables,
  /// or a dummy variable that will not be substituted.
  Const(LispVal),
  /// A free variable. This is an index into the [`Mm0Expr::subst`] array.
  Var(u32),
  /// A term constructor, where at least one subexpression is non-constant
  /// (else we would use [`Const`](Self::Const)).
  Expr(TermId, Vec<Mm0ExprNode>),
}

impl Remap for Mm0ExprNode {
  type Target = Self;
  fn remap(&self, r: &mut Remapper) -> Self {
    match self {
      Mm0ExprNode::Const(c) => Mm0ExprNode::Const(c.remap(r)),
      &Mm0ExprNode::Var(i) => Mm0ExprNode::Var(i),
      Mm0ExprNode::Expr(t, es) => Mm0ExprNode::Expr(t.remap(r), es.remap(r)),
    }
  }
}

struct Mm0ExprNodePrint<'a, T>(&'a [T], &'a Mm0ExprNode);
impl<'a, T: EnvDisplay> EnvDisplay for Mm0ExprNodePrint<'a, T> {
  fn fmt(&self, fe: FormatEnv<'_>, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self.1 {
      Mm0ExprNode::Const(e) => e.fmt(fe, f),
      &Mm0ExprNode::Var(i) => self.0[i as usize].fmt(fe, f),
      Mm0ExprNode::Expr(t, es) => {
        write!(f, "({}", fe.to(t))?;
        for e in es {
          write!(f, " {}", fe.to(&Self(self.0, e)))?
        }
        write!(f, ")")
      }
    }
  }
}

/// An embedded MM0 expression inside MMC. All free variables have been replaced by indexes,
/// with `subst` holding the internal names of these variables.
#[derive(Clone, Debug, DeepSizeOf)]
pub struct Mm0Expr<T> {
  /// The mapping from indexes in the `expr` to internal names.
  /// (The user-facing names have been erased.)
  pub subst: Vec<T>,
  /// The root node of the expression.
  pub expr: Rc<Mm0ExprNode>,
}

impl<T: std::hash::Hash> std::hash::Hash for Mm0Expr<T> {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    self.subst.hash(state);
    Rc::as_ptr(&self.expr).hash(state);
  }
}

impl<T: PartialEq> PartialEq for Mm0Expr<T> {
  fn eq(&self, other: &Self) -> bool {
    self.subst == other.subst && Rc::ptr_eq(&self.expr, &other.expr)
  }
}
impl<T: Eq> Eq for Mm0Expr<T> {}

impl<T: Remap> Remap for Mm0Expr<T> {
  type Target = Mm0Expr<T::Target>;
  fn remap(&self, r: &mut Remapper) -> Mm0Expr<T::Target> {
    Mm0Expr {subst: self.subst.remap(r), expr: self.expr.remap(r)}
  }
}

impl<T: EnvDisplay> EnvDisplay for Mm0Expr<T> {
  fn fmt(&self, fe: FormatEnv<'_>, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    Mm0ExprNodePrint(&self.subst, &self.expr).fmt(fe, f)
  }
}
