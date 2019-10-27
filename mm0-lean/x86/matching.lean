import x86.x86 data.set.lattice data.list.basic data.pfun data.list.alist

namespace x86

inductive flag_place | CFP | ZFP | SFP | OFP

def flag_place.read (f : flags) : flag_place → bool
| flag_place.CFP := f.CF
| flag_place.ZFP := f.ZF
| flag_place.SFP := f.SF
| flag_place.OFP := f.OF

def flag_place.write (f : flags) (b : bool) : flag_place → flags
| flag_place.CFP := {CF := b, ..f}
| flag_place.ZFP := {ZF := b, ..f}
| flag_place.SFP := {SF := b, ..f}
| flag_place.OFP := {OF := b, ..f}

inductive place
| flag : flag_place → place
| reg : regnum → place
| rip : place
| mem : qword → place

inductive place.read (k : config) : place → ∀ {n}, bitvec n → Prop
| flag {f} : place.read (place.flag f) (bitvec.singleton (f.read k.flags))
| reg {r} : place.read (place.reg r) (k.regs r)
| rip : place.read place.rip k.rip
| mem {a b} : k.mem.read1 perm.R a b → place.read (place.mem a) b

inductive place.write (k : config) : place → ∀ {n}, bitvec n → config → Prop
| flag {f b} : place.write (place.flag f) (bitvec.singleton b) {flags := f.write k.flags b, ..k}
| reg {r n q} : place.write (place.reg r) q (k.write_reg r n q)
| rip {q} : place.write place.rip q {rip := q, ..k}
| mem {a b m'} : k.mem.write1 a b m' → place.write (place.mem a) b {mem := m', ..k}

def place.stable (k k' : config) : place → Prop
| (place.flag f) := f.read k.flags = f.read k'.flags
| (place.reg r) := k.regs r = k'.regs r
| place.rip := k.rip = k'.rip
| (place.mem a) :=
  roption.mk (k.mem.valid a) (λ h, (k.mem.mem a h, k.mem.perm a h)) =
  roption.mk (k'.mem.valid a) (λ h, (k'.mem.mem a h, k'.mem.perm a h))

def stability (k k' : config) : set place := {p | place.stable k k' p}

def split (H h₁ h₂ : set place) : Prop :=
H = h₁ ∪ h₂ ∧ disjoint h₁ h₂

def sProp := config → Prop

def sProp.reserve (S : set place) : sProp :=
λ k, ∀ p ∈ S, ∃ n v, @place.read k p n v

def mProp := config → config → Prop

def mProp.exterior (P : mProp) : set place :=
{p | ∀ k k', P k k' → place.stable k k' p}

def stable_at (D : set place) : mProp := λ k k', D ⊆ stability k k'

def mProp.initial (Q : mProp) (P : sProp) : mProp := λ k k', Q k k' ∧ P k
def mProp.final (Q : mProp) (P : sProp) : mProp := λ k k', Q k k' ∧ P k'

def mProp.apply (Q : mProp) (P : sProp) : sProp :=
λ k', ∃ k, P k ∧ Q k k'

def mProp.id : mProp := eq

def mProp.comp (P Q : mProp) : mProp :=
λ k k₂, ∃ k₁, P k k₁ ∧ Q k₁ k₂

def slift (p : Prop) : sProp := λ _, p
def mlift (p : Prop) : mProp := λ _ _, p

def sProp.and (P Q : sProp) : sProp := λ k, P k ∧ Q k
def mProp.and (P Q : mProp) : mProp := λ k k', P k k' ∧ Q k k'
def sProp.or (P Q : sProp) : sProp := λ k, P k ∨ Q k
def mProp.or (P Q : mProp) : mProp := λ k k', P k k' ∨ Q k k'
instance lattice.has_inf.sProp : lattice.has_inf sProp := ⟨sProp.and⟩
instance lattice.has_inf.mProp : lattice.has_inf mProp := ⟨mProp.and⟩
instance lattice.has_sup.sProp : lattice.has_sup sProp := ⟨sProp.or⟩
instance lattice.has_sup.mProp : lattice.has_sup mProp := ⟨mProp.or⟩

instance lattice.has_top.sProp : lattice.has_top sProp := ⟨λ _, true⟩
instance lattice.has_top.mProp : lattice.has_top mProp := ⟨λ _ _, true⟩
instance lattice.has_bot.sProp : lattice.has_bot sProp := ⟨λ _, false⟩
instance lattice.has_bot.mProp : lattice.has_bot mProp := ⟨λ _ _, false⟩

def sProp.with (p : sProp) (q : Prop) : sProp := p ⊓ slift q
def mProp.with (p : mProp) (q : Prop) : mProp := p ⊓ mlift q

def sProp.ex {α} (p : α → sProp) : sProp := λ k, ∃ a, p a k
def mProp.ex {α} (p : α → mProp) : mProp := λ k k', ∃ a, p a k k'

def sProp.al {α} (p : α → sProp) : sProp := λ k, ∀ a, p a k
def mProp.al {α} (p : α → mProp) : mProp := λ k k', ∀ a, p a k k'

def sProp.sn (p : place) {n} (v : bitvec n) : sProp :=
λ k, p.read k v
def mProp.write (p : place) {n} (v' : bitvec n) : mProp :=
λ k k', p.write k v' k'
def mProp.sn (p : place) {n} (v v' : bitvec n) : mProp :=
(mProp.write p v').initial (sProp.sn p v)
def mProp.clob (p : place) {n} (v : bitvec n) : mProp :=
mProp.ex $ λ v', mProp.sn p v v'

def sProp.flags (f : flags) : sProp :=
sProp.al $ λ p : flag_place,
sProp.sn (place.flag p) (bitvec.singleton $ p.read f)

inductive block
| reg : regnum → wsize → block
| mem : qword → qword → block
| const : list byte → block

def block.size : block → qword
| (block.reg _ sz) := (sz.to_nat / 8 : ℕ)
| (block.mem _ sz) := sz
| (block.const l) := l.length

inductive block.places : block → set place
| reg {r sz} : block.places (block.reg r sz) (place.reg r)
| mem {a b sz : qword} :
  (bitvec.sub b a).to_nat ≤ sz.to_nat →
  block.places (block.mem a sz) (place.mem b)

def block.disj (b1 b2 : block) := disjoint b1.places b2.places

def sProp.mem_block (p : perm) (a : qword) (v : list byte) : sProp :=
λ k, k.mem.read' p a v

def block.read : block → list byte → sProp
| (block.reg r sz) v :=
  (sProp.reserve (block.reg r sz).places).and $ λ k,
    read_full_imm sz (k.regs r) v
| (block.mem a sz) v :=
  (sProp.reserve (block.mem a sz).places).and $ λ k,
    k.mem.read a v ∧ v.length = sz.to_nat
| (block.const l) v := slift $ v = l

def block.writable : block → sProp
| (block.reg r sz) := sProp.reserve (block.reg r sz).places
| (block.mem a sz) :=
  (sProp.reserve (block.mem a sz).places).and $ λ k,
    ∃ v, k.mem.read' perm.W a v ∧ v.length = sz.to_nat
| (block.const _) := slift false

def block.write : block → list byte → mProp
| (block.reg r sz) l k k' := ∃ v,
  @bits_to_byte (sz.to_nat * 8) sz.to_nat v l ∧
  mProp.write (place.reg r) v k k'
| (block.mem a sz) l k k' :=
  ∃ m', k.mem.write a l m' ∧ k' = {mem := m', ..k}
| (block.const _) _ _ _ := false

theorem block.read.size : ∀ {b v k},
  block.read b v k → v.length = b.size.to_nat := sorry

theorem block.write.size : ∀ {b v k k'},
  block.write b v k k' → v.length = b.size.to_nat := sorry

theorem block.read_write : ∀ {b v k k'},
  block.write b v k k' → block.read b v k' := sorry

theorem block.read.stable {l v k k'}
  (h₁ : block.read l v k) (ss : places l ⊆ stability k k') :
  block.read l v k' := sorry

def locals_ctx := alist (λ _ : ℕ, block)

def locals_ctx.get (Γ : locals_ctx) (n : ℕ) : option block := Γ.lookup n

def labels_ctx := qword × list qword

def labels_ctx.cons (q : qword) (L : labels_ctx) : labels_ctx :=
⟨L.1, q :: L.2⟩

inductive hoare_p (Q : kcfg → Prop) : kcfg → Prop
| zero {{k}} : Q k → hoare_p k
| step {{k}} : (∃ k', kcfg.step k k') →
  (∀ k', k.step k' → hoare_p k') → hoare_p k
| exit (k : kcfg) (ret) :
  k.k.exit ret → (ret = 0 → Q k) → hoare_p k

def hoare (P Q : kcfg → Prop) :=
∀ {{k}}, P k → hoare_p Q k

def sHoareIO (P : sProp) (Q : list byte → list byte → sProp) :=
∀ {{i o}}, hoare (λ k, P k.k ∧ k.input = i ∧ k.output = o)
  (λ k, ∃ i' o', i = i' ++ k.input ∧ o = k.output ++ o' ∧
    (Q i' o') k.k)
def mHoareIO (P : sProp) (Q : list byte → list byte → mProp) :=
sHoareIO P (λ i o, (Q i o).apply P)

def noIO (Q : mProp) (i o : list byte) : mProp := Q.with (i = [] ∧ o = [])
def mHoare (P : sProp) (Q : mProp) := mHoareIO P (noIO Q)

inductive exit_kind
| straight
| label (n : ℕ)

def exit_kind.result (L : labels_ctx) (pos : qword) : exit_kind → mProp
| exit_kind.straight := mProp.write place.rip pos
| (exit_kind.label n) := mProp.ex $ λ h, mProp.write place.rip (L.2.nth_le n h)

def stmt : Type :=
labels_ctx → locals_ctx → set place →
∀ rip : qword, list byte → Prop

def stmt.hoareIO (P : sProp) (C : stmt)
  (Q : exit_kind → list byte → list byte → mProp) : Prop :=
∀ L Γ D rip l, C L Γ D rip l →
  mHoareIO (P ⊓ sProp.sn place.rip rip ⊓ sProp.mem_block (perm.R + perm.X) rip l)
    (λ i o, stable_at D ⊓
      (mProp.write place.rip L.1 ⊔
       mProp.ex (λ e, Q e i o ⊓ e.result L (rip + l.length))))

def stmt.hoare (P : sProp) (C : stmt) (Q : exit_kind → mProp) : Prop :=
stmt.hoareIO P C (λ e, noIO (Q e))

def hstmt (P : sProp) (Q : mProp) : stmt :=
λ L Γ D rip v,
  mHoare (sProp.sn place.rip rip ⊓ sProp.mem_block (perm.R + perm.X) rip v)
    (Q ⊓ mProp.write place.rip (rip + v.length))

def stmt.all {α} (s : α → stmt) : stmt :=
λ L Γ D rip v, ∀ a, s a L Γ D rip v

def stmt.ex {α} (s : α → stmt) : stmt :=
λ L Γ D rip v, ∃ a, s a L Γ D rip v

def stmt.with (p : Prop) (s : stmt) : stmt :=
stmt.ex $ λ h : p, s

def stmt.or (s₁ s₂ : stmt) : stmt :=
λ L Γ D rip v, s₁ L Γ D rip v ∨ s₂ L Γ D rip v

def stmt.stabilize (S : set place) (s : stmt) : stmt :=
λ L Γ D, s L Γ (D ∪ S)

def expr := block → stmt

def expr.hoareIO (P : sProp) (E : expr)
  (Q : block → list byte → list byte → mProp) : Prop :=
∀ b, (E b).hoareIO P $
  λ e i o, exit_kind.cases_on e (Q b i o) (λ n, mlift false)

def expr.hoare (P : sProp) (E : expr) (Q : block → mProp) : Prop :=
expr.hoareIO P E (λ ret, noIO (Q ret))

def expr.stabilize (S : set place) (s : expr) : expr := λ b, (s b).stabilize S

class value (α : Type*) :=
(size : ℕ)
(eval : α → list byte → Prop)
(eval_eq : ∀ {{a l}}, eval a l → l.length = size)

def value.evalB {α} [value α] (x : α) (b : block) : sProp :=
λ k, ∃ l, block.read b l k ∧ value.eval x l

def value.read_sized {α} [value α] (a : qword) (x : α) (b : block) : sProp :=
value.evalB x b ⊓ slift (b = block.mem a (value.size α))

class type (α : Type*) :=
(size : ℕ)
(read : α → block → set place → sProp)
(write : α → block → mProp)
(read_eq : ∀ {{a l s k}}, read a l s k → l.size.to_nat = size)
(write_eq : ∀ {{a l k k'}}, write a l k k' → l.size.to_nat = size)
(read_write : ∀ {{a l k k'}}, write a l k k' → ∃ s, read a l s k')
(read_stable : ∀ {{a l s k k'}},
  read a l s k → s ⊆ stability k k' → read a l s k')

def type.read' {α} [type α] (a : α) (b : block) : sProp :=
sProp.ex $ type.read a b

instance (α) [value α] : type α :=
⟨value.size α,
  λ a b s, sProp.ex $ λ v,
    block.read b v ⊓ slift (value.eval a v ∧ s = b.places),
  λ a b, mProp.ex $ λ v,
    block.write b v ⊓ mlift (value.eval a v),
  λ a b s k ⟨v, h₁, h₂, _⟩, by rw [← h₁.size, @value.eval_eq α _ a v h₂],
  λ a b k k' ⟨v, h₁, h₂⟩, by rw [← h₁.size, @value.eval_eq α _ a v h₂],
  λ a v k k' ⟨s, h₁, h₂⟩, ⟨_, _, block.read_write h₁, h₂, rfl⟩,
  λ a l s k k' ⟨v, h₁, h₂, e⟩ ss, ⟨v, h₁.stable (e ▸ ss), h₂, e⟩⟩

def expr.hoareT (P : sProp) (E : expr) {α} [type α] (Q : α → set place → mProp) : Prop :=
expr.hoare P E $ λ v,
mProp.ex $ λ a, mProp.ex $ λ s, (Q a s).final (type.read a v s)

theorem bits_to_byte_length {n m w v} : @bits_to_byte n m w v → v.length = m :=
by rintro ⟨bs⟩; exact bs.2

def bits.value {n} (m : ℕ) : value (bitvec n) :=
⟨m, bits_to_byte m, λ a v h, bits_to_byte_length h⟩

instance unit.value : value unit :=
⟨0, λ _ v, v = [], by rintro _ _ ⟨⟩; refl⟩

instance byte.value : value byte :=
⟨1, λ b v, v = [b], by rintro _ _ ⟨⟩; refl⟩

instance word.value : value word := bits.value 4
instance qword.value : value qword := bits.value 8

class box (α) [type α] := (deref : α)

instance box.type (α) [type α] : type (box α) :=
⟨8, λ x b s k, ∃ a l s',
    block.read b l k ∧
    qword.to_list_byte a l ∧
    type.read x.deref (block.mem a (type.size α)) s' k ∧
    s = b.places ∪ s',
  λ x b k k', ∃ a l s',
    block.write b l k k' ∧
    qword.to_list_byte a l ∧
    type.read x.deref (block.mem a (type.size α)) s' k',
  λ x b s k ⟨a, l, s', h₁, h₂, _⟩,
    by rw [← h₁.size, bits_to_byte_length h₂],
  λ x b k k' ⟨a, l, s', h₁, h₂, _⟩,
    by rw [← h₁.size, bits_to_byte_length h₂],
  λ x b k k' ⟨a, l, s', h₁, h₂, h₃⟩,
    ⟨_, _, _, _, block.read_write h₁, h₂, h₃, rfl⟩,
  λ x b s k k' ⟨a, l, s', h₁, h₂, h₃, e⟩ ss,
    let ⟨ss₁, ss₂⟩ := set.union_subset_iff.1 (by rwa e at ss) in
    ⟨a, l, s', h₁.stable ss₁, h₂, type.read_stable h₃ ss₂, e⟩⟩

def ret (b : block) : expr :=
λ bl L Γ D a v, bl = b ∧ v = []

def const (l : list byte) : expr := ret (block.const l)

def var (i : ℕ) : expr :=
λ bl L Γ D a v, bl ∈ Γ.get i ∧ v = []

def hexpr (P : sProp) (Q : block → mProp) : expr :=
λ ret, hstmt P (Q ret)

def expr.all {α} (e : α → expr) : expr :=
λ ret, stmt.all $ λ a, e a ret

def expr.ex {α} (e : α → expr) : expr :=
λ ret, stmt.ex $ λ a, e a ret

def expr.with (p : Prop) (s : expr) : expr :=
expr.ex $ λ h : p, s

def const' {α} [type α] (a : α) : expr :=
hexpr ⊤ $ λ b, mProp.final mProp.id (type.read' a b)

inductive stmt.seq (s₁ : stmt) (s₂ : stmt) : stmt
| mk {L Γ D rip v₁ v₂} :
  s₁ L Γ D rip v₁ →
  s₂ L Γ D (rip + v₁.length) v₂ →
  stmt.seq L Γ D rip (v₁ ++ v₂)

inductive expr.bindS (e₁ : expr) (s₂ : block → stmt) : stmt
| mk {b L Γ D rip v₁ v₂} :
  e₁ b L Γ D rip v₁ →
  s₂ b L Γ D (rip + v₁.length) v₂ →
  expr.bindS L Γ D rip (v₁ ++ v₂)

def expr.bind (e₁ : expr) (e₂ : block → expr) : expr :=
λ b₂, expr.bindS e₁ $ λ b₁, e₂ b₁ b₂

def block.mov (dst src : block) : stmt :=
stmt.with (dst.size = src.size) $
stmt.all $ λ val, hstmt (block.read src val) (block.write dst val)

def expr.set (e₁ e₂ : expr) : stmt :=
expr.bindS e₁ $ λ dst, expr.bindS e₂ $ λ src, block.mov dst src

inductive label | fail | label (n : ℕ)

inductive label.loc (L : labels_ctx) : label → qword → Prop
| fail : label.loc label.fail L.1
| label (n h) : label.loc (label.label n) (L.2.nth_le n h)

def stmt.jump_cc (p : flags → bool) (l : label) : stmt :=
λ L Γ D rip v, ∀ tgt, l.loc L tgt →
mHoare (sProp.sn place.rip rip ⊓ sProp.mem_block (perm.R + perm.X) rip v)
  (λ k k', stable_at D k k' ∧
    mProp.write place.rip (cond (p k.flags) tgt (rip + v.length)) k k')

def stmt.jump : label → stmt := stmt.jump_cc (λ _, tt)

def boolexpr := (flags → bool) → stmt

def boolexpr.hoare (P : sProp) (E : boolexpr) (Q : bool → mProp) : Prop :=
∃ p, stmt.hoare P (E p)
  (λ e k k', e = exit_kind.straight ∧ Q (p k'.flags) k k')

def boolexpr.not (c : boolexpr) : boolexpr :=
λ p, c (bnot ∘ p)

def boolexpr.jump_if (c : boolexpr) (l : label) : stmt :=
stmt.ex $ λ p, (c p).seq $ stmt.jump_cc p l

def stmt.nop : stmt := λ L Γ D rip v, v = []

def if_stmt (c : boolexpr) (s₁ s₂ : stmt) : stmt :=
stmt.ex $ λ p, stmt.seq (c p) $ λ L Γ D rip v,
∃ v₁ v₂ v₃, v = v₁ ++ v₂ ++ v₃ ∧
let q₁ := rip + v₁.length, q₂ := q₁ + v₂.length in
stmt.jump_cc (bnot ∘ p) (label.label 0) (L.cons q₁) Γ D rip v₁ ∧
s₁.seq (stmt.jump (label.label 0)) (L.cons q₂) Γ D q₁ v₂ ∧
s₂.seq (stmt.jump (label.label 0)) (L.cons q₂) Γ D q₂ v₃

def loop (s : stmt) : stmt :=
λ L Γ D rip, s (L.cons rip) Γ D rip

def block_stmt (s : stmt) : stmt :=
λ L Γ D rip v, s (L.cons (rip + v.length)) Γ D rip v

def while (c : boolexpr) (s : stmt) : stmt :=
block_stmt $ loop $
  (c.not.jump_if (label.label 1)).seq $
  s.seq $
  stmt.jump (label.label 0)

def decl_block (b : block) (s : ℕ → stmt) : stmt :=
λ L Γ D rip v, ∃ i, s i L (Γ.insert i b) D rip v

def decl (sz : qword) (s : ℕ → stmt) : stmt :=
stmt.ex $ λ b, stmt.with (block.size b = sz) $ decl_block b s

def init (e : expr) (s : ℕ → stmt) : stmt :=
e.bindS $ λ b, decl_block b s

def binop_expr {α β γ} [type α] [type β] [type γ]
  (f : α → β → γ) (e₁ e₂ : expr) : expr :=
e₁.bind $ λ b₁, e₂.bind $ λ b₂ b, stmt.ex $ λ x, stmt.ex $ λ y,
hstmt (type.read' x b₁ ⊓ type.read' y b₂) (type.write (f x y) b)

def asn_binop {α β} [type α] [type β] (f : α → β → α) (e₁ e₂ : expr) : stmt :=
e₁.bindS $ λ b₁, (ret b₁).set (binop_expr f (ret b₁) e₂)

def unop_expr {α β} [type α] [type β]
  (f : α → β) (e : expr) : expr :=
e.bind $ λ b₁ b, stmt.ex $ λ x,
hstmt (type.read' x b₁) (type.write (f x) b)

def asn_unop {α} [type α] (f : α → α) (e : expr) : stmt :=
e.bindS $ λ b, (ret b).set (unop_expr f (ret b))

def for (start : expr) (test : ℕ → boolexpr) (incr body : ℕ → stmt) : stmt :=
init start $ λ i, while (test i) $ (body i).seq (incr i)

def incr (α) [type α] [has_add α] [has_one α] : expr → stmt :=
asn_unop (+ (1 : α))

def bool_binop {α β} [type α] [type β]
  (f : α → β → bool) (e₁ e₂ : expr) : boolexpr :=
λ p, e₁.bindS $ λ b₁, e₂.bindS $ λ b₂, stmt.ex $ λ x, stmt.ex $ λ y,
hstmt (type.read' x b₁ ⊓ type.read' y b₂)
  (mProp.final mProp.id (λ k, p k.flags = f x y))

def lt (e₁ e₂ : expr) : boolexpr :=
bool_binop (λ a b : qword, a.to_nat < b.to_nat) e₁ e₂

def for_seq (sz : qword) (max : expr) (body : ℕ → stmt) : stmt :=
for (const' (0 : qword)) (λ i, lt (const' (0 : qword)) max) (λ i, incr qword (var i)) body

end x86