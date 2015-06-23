use value::Rvalue;
use mnemonic::{Mnemonic,Bound};
use guard::Guard;
use std::rc::Rc;
use num::traits::*;
use std::fmt::Debug;
use std::slice::Iter;
use std::ops::{BitAnd,BitOr,Shl,Not};
use std::collections::HashMap;
use std::mem::size_of;
use codegen::CodeGen;
use layer::LayerIter;

pub trait Token: Clone + Zero + One + Debug + Not + BitOr + BitAnd + Shl<usize> + NumCast
where <Self as Not>::Output: NumCast,
      <Self as BitOr>::Output: NumCast,
      <Self as BitAnd>::Output: NumCast,
      <Self as Shl<usize>>::Output: NumCast
{}

impl Token for u8 {}

pub type Action<I: Token> = fn(&mut State<I>) -> bool;

#[derive(Debug)]
pub struct State<I: Clone> {
    // in
    pub address: u64,
    pub tokens: Vec<I>,
    pub groups: Vec<(String,I)>,

    // out
    pub mnemonics: Vec<Mnemonic>,
    pub jumps: Vec<(Rvalue,Guard)>,

    next_address: u64,
}

impl<I: Clone> State<I> {
    pub fn new(a: u64) -> State<I> {
        State{
            address: a,
            tokens: vec!(),
            groups: vec!(),
            mnemonics: Vec::new(),
            jumps: Vec::new(),
            next_address: a,
        }
    }

    pub fn mnemonic<F: Fn(&CodeGen) -> ()>(&mut self,len: usize, n: &str, fmt: &str, ops: Vec<Rvalue>, f: F) {
        self.mnemonic_dynargs(len,n,fmt,|cg: &CodeGen| -> Vec<Rvalue> {
            f(cg);
            ops.clone()
        });
    }

    pub fn mnemonic_dynargs<F>(&mut self,len: usize, n: &str, fmt: &str, f: F)
    where F: Fn(&CodeGen) -> Vec<Rvalue> {
        let mut cg = CodeGen::new();
        let ops = f(&cg);

        self.mnemonics.push(Mnemonic::new(
                self.next_address..(self.next_address + (len as u64)),
                n.to_string(),
                fmt.to_string(),
                ops.iter(),
                cg.instructions.iter()));
        self.next_address += len as u64;
    }

    pub fn jump(&mut self,v: Rvalue,g: Guard) {
        self.jumps.push((v,g));
    }
}

#[derive(Clone)]
pub struct Match<I: Token> {
    patterns: Vec<(I,I)>,
    actions: Vec<Rc<Action<I>>>,
    groups: Vec<(String,Vec<I>)>
}

pub enum Expr<I: Token> {
    Pattern(String),
    Terminal(I),
    Subdecoder(Rc<Disassembler<I>>)
}

pub trait ToExpr<I: Token> {
    fn to_expr(&self) -> Expr<I>;
}

impl<'a,I: Token> ToExpr<I> for &'a str {
    fn to_expr(&self) -> Expr<I> {
        Expr::Pattern(self.to_string())
    }
}

impl<'a,I: Token> ToExpr<I> for Rc<Disassembler<I>> {
    fn to_expr(&self) -> Expr<I> {
        Expr::Subdecoder(self.clone())
    }
}

impl<I: Token> ToExpr<I> for usize {
    fn to_expr(&self) -> Expr<I> {
        Expr::Terminal(I::from::<usize>(*self).unwrap().clone())
    }
}

impl<I: Token> Expr<I> {
    pub fn matches(&self) -> Vec<Match<I>>
    where <I as Not>::Output: NumCast,
          <I as BitOr>::Output: NumCast,
          <I as BitAnd>::Output: NumCast,
          <I as Shl<usize>>::Output: NumCast
    {
        let mut pats = Vec::<(I,I)>::new();
        let mut grps = HashMap::<String,Vec<I>>::new();

        match self {
            &Expr::Pattern(ref s) => {
                let mut groups = HashMap::<String,I>::new();
                let mut cur_group = "".to_string();
                let mut read_pat = true; // false while reading torwards @
                let mut bit = size_of::<I>() * 8;
                let mut invmask = I::zero();
                let mut pat = I::zero();

                for c in s.chars() {
                    match c {
                        '@' => {
                            if read_pat {
                                error!("Pattern syntax error: read '@' w/o name in '{}'",s);
                                return Vec::new();
                            } else {
                                read_pat = true;

                                if cur_group == "" {
                                    error!("Pattern syntax error: anonymous groups not allowed in '{}'",s);
                                    return Vec::new();
                                }

                                groups.insert(cur_group.clone(),I::zero());
                            }
                        },
                        ' ' => (),
                        '.' => {
                            if read_pat {
                                invmask = cast(invmask | cast(I::one() << (bit - 1)).unwrap()).unwrap();
                                bit -= 1;
                            } else {
                                error!("Pattern syntax error: read '.' while expecting '@' in '{}'",s);
                                return Vec::new();
                            }
                        },
                        '0'...'1' => {
                            if read_pat {
                                if c == '1' {
                                    pat = cast(pat | cast(I::one() << (bit - 1)).unwrap()).unwrap();
                                }

                                if cur_group != "" {
                                    *groups.get_mut(&cur_group).unwrap() = cast(groups.get(&cur_group).unwrap().clone() | cast(I::one() << (bit - 1)).unwrap()).unwrap();
                                }

                                bit -= 1;
                            } else {
                                error!("Pattern syntax error: pattern start without '@' delimiter in '{}'",s);
                                return Vec::new();
                            }
                        },
                        'a'...'z' | 'A'...'Z' => {
                            if read_pat {
                                cur_group = c.to_string();
                                read_pat = false;
                            } else {
                                cur_group.push(c);
                            }
                        },
                        _ => {
                            error!("Pattern syntax error: invalid character '{}' in '{}'",c,s);
                            return Vec::new();
                        }
                    }
                }

                if bit != 0 {
                    error!("Pattern syntax error: invalid pattern length");
                    return Vec::new();
                }

                pats.push((pat,cast(!invmask).unwrap()));

                for g in groups {
                    if grps.contains_key(&g.0) {
                        grps.get_mut(&g.0).unwrap().push(g.1)
                    } else {
                        grps.insert(g.0,vec!(g.1));
                    }
                }
            },
            &Expr::Terminal(ref i) => pats.push((i.clone(),cast(!I::zero()).unwrap())),
            &Expr::Subdecoder(ref m) => return m.matches.clone(),
        }

        vec!(Match::<I>{
            patterns: pats,
            groups: grps.iter().map(|x| (x.0.clone(),x.1.clone())).collect(),
            actions: vec!()
        })
    }
}

pub struct Disassembler<I: Token> {
    matches: Vec<Match<I>>,
    default: Option<Action<I>>,
}

impl<I: Token> Disassembler<I> {
    pub fn new() -> Disassembler<I> {
        Disassembler::<I> {
            matches: Vec::new(),
            default: None,
        }
    }

    pub fn set_default(&mut self,f: Action<I>) {
        self.default = Some(f);
    }

    fn combine_expr(mut i: Iter<Expr<I>>, a: Action<I>) -> Vec<Match<I>>
    where <I as Not>::Output: NumCast,
          <I as BitOr>::Output: NumCast,
          <I as BitAnd>::Output: NumCast,
          <I as Shl<usize>>::Output: NumCast
    {
        match i.next() {
            Some(e) => {
                let mut rest = Self::combine_expr(i,a);
                let mut ret = Vec::new();


                for mut _match in (*e).matches() {
                    for pre in &rest {
                        for x in &pre.patterns {
                            _match.patterns.push(x.clone());
                        }

                        for x in &pre.actions {
                            _match.actions.push(x.clone());
                        }
                        for x in &pre.groups {
                            for y in _match.groups.iter_mut() {
                                if y.0 == x.0 {
                                    for p in &x.1 {
                                        y.1.push(p.clone());
                                    }
                                }
                            }
                        }
                    }

                    ret.push(Match::<I>{
                        patterns: _match.patterns,
                        actions:_match.actions,
                        groups: _match.groups
                    });
                }

                ret
            },
            None => Vec::new()
        }
    }

    pub fn add_expr(&mut self, e: Vec<Expr<I>>, a: Action<I>)
    where <I as Not>::Output: NumCast,
          <I as BitAnd>::Output: NumCast,
          <I as BitOr>::Output: NumCast,
          <I as Shl<usize>>::Output: NumCast
    {
        for x in Self::combine_expr(e.iter(),a) {
            self.matches.push(x);
        }
    }
/*
    template<typename Tag>
	boost::optional<std::pair<slab::iterator,sem_state<Tag>>> disassembler<Tag>::try_match(slab::iterator b, slab::iterator e,sem_state<Tag> const& _st) const
	{
		using token = typename architecture_traits<Tag>::token_type;

		std::list<token> read;
		size_t const len = std::distance(b,e);
		std::function<boost::optional<token>(void)> read_next;

		read_next = [&](void) -> boost::optional<token>
		{
			auto i = b + read.size() * sizeof(token);
			bool const defined = std::none_of(i,i + sizeof(token),[](po::tryte s) { return !s; });

			if(!defined)
				return boost::none;

			std::array<uint8_t,sizeof(token)> tmp;

			std::transform(i,i + sizeof(token),tmp.begin(),[](po::tryte b) { return *b; });
			return std::accumulate(tmp.rbegin(),tmp.rend(),0,[](token acc, uint8_t b) { return (acc << 8) | b; });
		};

		if(len > 0)
		{
			for(auto const& opt: _pats)
			{
				auto const& pattern = opt.patterns;
				auto const& actions = opt.sem_actions;

				if(len < pattern.size() * sizeof(token))
					continue;

				while(read.size() < pattern.size())
				{
					auto maybe_token = read_next();

					if(!maybe_token)
						break;
					else
						read.push_back(*maybe_token);
				}

				if(read.size() < pattern.size())
					continue;

				auto j = pattern.begin();
				auto k = read.begin();
				bool match = true;

				while(match && j != pattern.end())
				{
					ensure(k != read.end());

					match &= (j->first & *k) == j->second;
					++j;
					++k;
				}

				if(match)
				{
					sem_state<Tag> st(_st);

					for(auto cap: opt.cap_groups)
					{
						std::list<token> masks = cap.second;
						uint64_t res;

						ensure(masks.size() == pattern.size());

						if(st.capture_groups.count(cap.first))
						{
							res = st.capture_groups.at(cap.first);
							st.capture_groups.erase(cap.first);
						}
						else
						{
							res = 0;
						}

						auto t = read.begin();
						for(auto cg_mask: masks)
						{
							if(cg_mask == 0)
							{
								++t;
								continue;
							}

							ensure(t != k);
							int bit = sizeof(token) * 8 - 1;
							while(bit >= 0)
							{
								if((cg_mask >> bit) & 1)
									res = (res << 1) | ((*t >> bit) & 1);
								--bit;
							}

							++t;
						}

						st.capture_groups.emplace(cap.first,res);
					}

					std::copy(read.begin(),k,std::back_inserter(st.tokens));
					match = std::all_of(actions.begin(),actions.end(),[&](std::function<bool(sem_state<Tag>&)> fn) { return fn(st); });

					if(match)
						return std::make_pair(b + pattern.size() * sizeof(token),st);
				}
			}

			if(_default)
			{
				sem_state<Tag> st(_st);

				if(read.empty())
				{
					auto maybe_token = read_next();

					ensure(maybe_token);
					read.push_back(*maybe_token);
				}

				st.tokens.push_back(read.front());
				if((*_default)(st))
					return std::make_pair(b + sizeof(token),st);
			}
		}

		return boost::none;
	}*/
    pub fn next_match(&self,i: &mut LayerIter, st: State<I>) -> Option<State<I>>
    where <I as Not>::Output: NumCast,
          <I as BitAnd>::Output: NumCast,
          <I as BitOr>::Output: NumCast,
          <I as Shl<usize>>::Output: NumCast,
          I: Eq
    {
        let mut tokens = Vec::<I>::new();
        let mut j = i.clone();
        let min_len = |len: usize, ts: &mut Vec<I>, j: &mut LayerIter| -> bool {
            if ts.len() >= len {
                true
            } else {
                for t in j.take(len * size_of::<I>()) {
                    let mut tmp: I = I::zero();

                    for _ in (0..(size_of::<I>())) {
                        if let Some(b) = t {
                            tmp = cast(cast::<<I as Shl<usize>>::Output,I>(tmp << 8).unwrap() | cast(b).unwrap()).unwrap();
                        } else {
                            return false;
                        }
                    }
                    ts.push(tmp);
                }

                (ts.len() >= len)
            }
        };

        for opt in &self.matches {
            let pattern = &opt.patterns;
            let actions = &opt.actions;

            if !min_len(pattern.len(),&mut tokens,&mut j) {
                continue;
            }

            let is_match = pattern.iter().zip(tokens.iter()).all(|p| {
                cast::<<I as BitAnd>::Output,I>((p.0).clone().0 & p.1.clone()).unwrap() == (p.0).1
            });

            if is_match {
                unimplemented!();
            }
        }
        None
    }
}

macro_rules! new_disassembler {
    ($ty:ty => $( [ $( $t:expr ),+ ] = $f:expr),+) => {
        {
            let mut dis = Disassembler::<$ty>::new();

            $({
                let mut __x = Vec::new();
                $(
                    __x.push($t.to_expr());
                )+
                fn a(a: &mut State<$ty>) -> bool { ($f)(a) };
                let fuc: Action<$ty> = a;
                dis.add_expr(__x,fuc);
            })+

            Rc::<Disassembler<$ty>>::new(dis)
        }
    };
    ($ty:ty => $( [ $( $t:expr ),+ ] = $f:expr),+, _ = $def:expr) => {
        {
            let mut dis = Disassembler::<$ty>::new();

            $({
                let mut __x = Vec::new();
                $(
                    __x.push($t.to_expr());
                )+
                fn a(a: &mut State<$ty>) -> bool { ($f)(a) };
                let fuc: Action<$ty> = a;
                dis.add_expr(__x,fuc);
            })+

            fn __def(st: &mut State<u8>) -> bool { ($def)(st) };
            dis.set_default(__def);

            Rc::<Disassembler<$ty>>::new(dis)
        }
    };

}
/*

TEST_F(disassembler,sub_decoder)
{
	po::sem_state<test_tag> st(1,'a');
	boost::optional<std::pair<po::slab::iterator,po::sem_state<test_tag>>> i;

	i = main.try_match(bytes.begin()+1,bytes.end(),st);
	ASSERT_TRUE(!!i);
	st = i->second;

	ASSERT_EQ(std::distance(bytes.begin(), i->first), 3);
	ASSERT_EQ(st.address, 1u);
	ASSERT_GE(st.tokens.size(), 2u);
	ASSERT_EQ(st.tokens[0], 'A');
	ASSERT_EQ(st.tokens[1], 'B');
	ASSERT_EQ(st.capture_groups.size(), 0u);
	ASSERT_EQ(st.mnemonics.size(), 1u);
	ASSERT_EQ(st.mnemonics.front().opcode, std::string("BA"));
	ASSERT_EQ(st.mnemonics.front().area, po::bound(1,3));
	ASSERT_TRUE(st.mnemonics.front().instructions.empty());
	ASSERT_EQ(st.jumps.size(), 1u);
	ASSERT_TRUE(is_constant(st.jumps.front().first));
	ASSERT_EQ(to_constant(st.jumps.front().first).content(), 3u);
	ASSERT_TRUE(st.jumps.front().second.relations.empty());
}

TEST_F(disassembler,semantic_false)
{
	po::sem_state<test_tag> st(6,'a');
	boost::optional<std::pair<po::slab::iterator,po::sem_state<test_tag>>> i;

	i = main.try_match(bytes.begin()+6,bytes.end(),st);
	ASSERT_FALSE(!!i);
}

TEST_F(disassembler,default_pattern)
{
	po::sem_state<test_tag> st(7,'a');
	boost::optional<std::pair<po::slab::iterator,po::sem_state<test_tag>>> i;

	i = main.try_match(bytes.begin()+7,bytes.end(),st);
	ASSERT_TRUE(!!i);
	st = i->second;

	ASSERT_EQ(i->first, bytes.end());
	ASSERT_EQ(st.address, 7u);
	ASSERT_EQ(st.tokens.size(), 1u);
	ASSERT_EQ(st.tokens[0], 'X');
	ASSERT_EQ(st.capture_groups.size(), 0u);
	ASSERT_EQ(st.mnemonics.size(), 1u);
	ASSERT_EQ(st.mnemonics.front().opcode, std::string("UNK"));
	ASSERT_EQ(st.mnemonics.front().area, po::bound(7,8));
	ASSERT_TRUE(st.mnemonics.front().instructions.empty());
	ASSERT_EQ(st.jumps.size(), 1u);
	ASSERT_TRUE(is_constant(st.jumps.front().first));
	ASSERT_TRUE(st.jumps.front().second.relations.empty());
	ASSERT_EQ(to_constant(st.jumps.front().first).content(), 8u);
}

TEST_F(disassembler,slice)
{
	po::sem_state<test_tag> st(1,'a');
	boost::optional<std::pair<po::slab::iterator,po::sem_state<test_tag>>> i;

	i = main.try_match(bytes.begin()+1,bytes.begin()+2,st);
	ASSERT_TRUE(!!i);
	st = i->second;

	ASSERT_EQ(i->first, bytes.begin()+2);
	ASSERT_EQ(st.address, 1u);
	ASSERT_GE(st.tokens.size(), 1u);
	ASSERT_EQ(st.tokens[0], 'A');
	ASSERT_EQ(st.capture_groups.size(), 0u);
	ASSERT_EQ(st.mnemonics.size(), 1u);
	ASSERT_EQ(st.mnemonics.front().opcode, std::string("A"));
	ASSERT_EQ(st.mnemonics.front().area, po::bound(1,2));
	ASSERT_TRUE(st.mnemonics.front().instructions.empty());
	ASSERT_EQ(st.jumps.size(), 1u);
	ASSERT_TRUE(is_constant(st.jumps.front().first));
	ASSERT_TRUE(st.jumps.front().second.relations.empty());
	ASSERT_EQ(to_constant(st.jumps.front().first).content(), 2u);
}

TEST_F(disassembler,empty)
{
	po::sem_state<test_tag> st(0,'a');
	boost::optional<std::pair<po::slab::iterator,po::sem_state<test_tag>>> i;

	i = main.try_match(bytes.begin(),bytes.begin(),st);

	ASSERT_TRUE(!i);
	ASSERT_EQ(st.address, 0u);
	ASSERT_EQ(st.tokens.size(), 0u);
	ASSERT_EQ(st.capture_groups.size(), 0u);
	ASSERT_EQ(st.mnemonics.size(), 0u);
	ASSERT_EQ(st.jumps.size(), 0u);
}

TEST_F(disassembler,capture_group)
{
	po::sem_state<test_tag> st(4,'a');
	boost::optional<std::pair<po::slab::iterator,po::sem_state<test_tag>>> i;

	i = main.try_match(bytes.begin()+4,bytes.end(),st);
	ASSERT_TRUE(!!i);
	st = i->second;

	ASSERT_EQ(i->first, bytes.begin()+5);
	ASSERT_EQ(st.address, 4u);
	ASSERT_GE(st.tokens.size(), 1u);
	ASSERT_EQ(st.tokens[0], 'C');
	ASSERT_EQ(st.capture_groups.size(), 1u);
	ASSERT_EQ(st.capture_groups.count("k"), 1u);
	ASSERT_EQ(st.capture_groups["k"], 16u);
	ASSERT_EQ(st.mnemonics.size(), 1u);
	ASSERT_EQ(st.mnemonics.front().opcode, std::string("C"));
	ASSERT_EQ(st.mnemonics.front().area, po::bound(4,5));
	ASSERT_TRUE(st.mnemonics.front().instructions.empty());
	ASSERT_EQ(st.jumps.size(), 1u);
	ASSERT_TRUE(is_constant(st.jumps.front().first));
	ASSERT_TRUE(st.jumps.front().second.relations.empty());
	ASSERT_EQ(to_constant(st.jumps.front().first).content(), 5u);
}

TEST_F(disassembler,empty_capture_group)
{
	po::sem_state<test_tag> st(0,'a');
	std::vector<unsigned char> _buf = {127};
	po::slab buf(_buf.data(),_buf.size());
	po::disassembler<test_tag> dec;

	dec["01 a@.. 1 b@ c@..."] = [](ss s) { s.mnemonic(1, "1"); return true; };
	boost::optional<std::pair<po::slab::iterator,po::sem_state<test_tag>>> i;

	i = dec.try_match(buf.begin(),buf.end(),st);
	ASSERT_TRUE(!!i);
	st = i->second;

	ASSERT_EQ(std::distance(buf.begin(), i->first),1);
	ASSERT_EQ(st.address, 0u);
	ASSERT_EQ(st.tokens.size(), 1u);
	ASSERT_EQ(st.tokens[0], 127);
	ASSERT_EQ(st.capture_groups.size(), 2u);
	ASSERT_EQ(st.capture_groups.count("a"), 1u);
	ASSERT_EQ(st.capture_groups.count("c"), 1u);
	ASSERT_EQ(st.capture_groups["a"], 3u);
	ASSERT_EQ(st.capture_groups["c"], 7u);
	ASSERT_EQ(st.mnemonics.size(), 1u);
	ASSERT_EQ(st.mnemonics.front().opcode, std::string("1"));
	ASSERT_EQ(st.mnemonics.front().area, po::bound(0,1));
	ASSERT_TRUE(st.mnemonics.front().instructions.empty());
	ASSERT_EQ(st.jumps.size(), 0u);
}

TEST_F(disassembler,too_long_capture_group)
{
	po::sem_state<test_tag> st(0,'a');
	std::vector<unsigned char> buf = {127};
	po::disassembler<test_tag> dec;

	ASSERT_THROW(dec["k@........."],po::tokpat_error);
}

TEST_F(disassembler,too_long_token_pattern)
{
	po::sem_state<test_tag> st(0,'a');
	std::vector<unsigned char> buf = {127};
	po::disassembler<test_tag> dec;

	ASSERT_THROW(dec["111111111"],po::tokpat_error);
}

TEST_F(disassembler,too_short_token_pattern)
{
	po::sem_state<test_tag> st(0,'a');
	std::vector<unsigned char> _buf = {127};
	po::slab buf(_buf.data(),_buf.size());
	po::disassembler<test_tag> dec;

	dec["1111111"];

	ASSERT_TRUE(!!dec.try_match(buf.begin(),buf.end(),st));
}

TEST_F(disassembler,invalid_token_pattern)
{
	po::sem_state<test_tag> st(0,'a');
	std::vector<unsigned char> buf = {127};
	po::disassembler<test_tag> dec;

	ASSERT_THROW(dec["a111111"];,po::tokpat_error);
}

using sw = po::sem_state<wtest_tag>&;

TEST_F(disassembler,wide_token)
{
	po::sem_state<wtest_tag> st(0,'a');
	std::vector<uint8_t> _buf = {0x22,0x11, 0x44,0x33, 0x44,0x55};
	po::slab buf(_buf.data(),_buf.size());
	po::disassembler<wtest_tag> dec;

	dec[0x1122] = [](sw s)
	{
		s.mnemonic(2,"A");
		s.jump(s.address + 2);
		return true;
	};

	dec[0x3344] = [](sw s)
	{
		s.mnemonic(2,"B");
		s.jump(s.address + 2);
		s.jump(s.address + 4);
		return true;
	};

	dec[0x5544] = [](sw s)
	{
		s.mnemonic(2, "C");
		return true;
	};

	boost::optional<std::pair<po::slab::iterator,po::sem_state<wtest_tag>>> i;

	i = dec.try_match(buf.begin(),buf.end(),st);
	ASSERT_TRUE(!!i);
	st = i->second;

	ASSERT_EQ(std::distance(buf.begin(), i->first),2);
	ASSERT_EQ(st.address, 0u);
	ASSERT_EQ(st.tokens.size(), 1u);
	ASSERT_EQ(st.tokens[0], 0x1122u);
	ASSERT_EQ(st.mnemonics.size(), 1u);
	ASSERT_EQ(st.mnemonics.front().opcode, std::string("A"));
	ASSERT_EQ(st.mnemonics.front().area, po::bound(0,2));
	ASSERT_TRUE(st.mnemonics.front().instructions.empty());
	ASSERT_EQ(st.jumps.size(), 1u);
}

TEST_F(disassembler,optional)
{

	po::sem_state<test_tag> st(0,'a');
	std::vector<unsigned char> _buf = {127,126,125,127,125};
	po::slab buf(_buf.data(),_buf.size());
	po::disassembler<test_tag> dec;

	dec[po::token_expr(127) >> *po::token_expr(126) >> po::token_expr(125)] = [](ss s) { s.mnemonic(s.tokens.size(), "1"); return true; };
	boost::optional<std::pair<po::slab::iterator,po::sem_state<test_tag>>> i;

	i = dec.try_match(buf.begin(),buf.end(),st);
	ASSERT_TRUE(!!i);
	st = i->second;

	ASSERT_EQ(std::distance(buf.begin(), i->first),3);
	ASSERT_EQ(st.address, 0u);
	ASSERT_EQ(st.tokens.size(), 3u);
	ASSERT_EQ(st.tokens[0], 127u);
	ASSERT_EQ(st.tokens[1], 126u);
	ASSERT_EQ(st.tokens[2], 125u);
	ASSERT_EQ(st.capture_groups.size(), 0u);
	ASSERT_EQ(st.mnemonics.size(), 1u);
	ASSERT_EQ(st.mnemonics.front().opcode, std::string("1"));
	ASSERT_EQ(st.mnemonics.front().area, po::bound(0,3));
	ASSERT_TRUE(st.mnemonics.front().instructions.empty());
	ASSERT_EQ(st.jumps.size(), 0u);

	st = po::sem_state<test_tag>(3,'a');
	i = dec.try_match(i->first,buf.end(),st);
	ASSERT_TRUE(!!i);
	st = i->second;

	ASSERT_EQ(std::distance(buf.begin(), i->first),5);
	ASSERT_EQ(st.address, 3u);
	ASSERT_EQ(st.tokens.size(), 2u);
	ASSERT_EQ(st.tokens[0], 127u);
	ASSERT_EQ(st.tokens[1], 125u);
	ASSERT_EQ(st.capture_groups.size(), 0u);
	ASSERT_EQ(st.mnemonics.size(), 1u);
	ASSERT_EQ(st.mnemonics.front().opcode, std::string("1"));
	ASSERT_EQ(st.mnemonics.front().area, po::bound(3,5));
	ASSERT_TRUE(st.mnemonics.front().instructions.empty());
	ASSERT_EQ(st.jumps.size(), 0u);
}

TEST_F(disassembler,fixed_capture_group_contents)
{

	po::sem_state<test_tag> st(0,'a');
	std::vector<unsigned char> _buf = {127,255};
	po::slab buf(_buf.data(),_buf.size());
	po::disassembler<test_tag> dec;

	dec[ po::token_expr(std::string("01111111")) >> po::token_expr(std::string("a@11111111")) ] = [](ss s) { s.mnemonic(1,"1"); return true; };
	boost::optional<std::pair<po::slab::iterator,po::sem_state<test_tag>>> i;

	i = dec.try_match(buf.begin(),buf.end(),st);
	ASSERT_TRUE(!!i);
	st = i->second;

	ASSERT_EQ(std::distance(buf.begin(), i->first),2);
	ASSERT_EQ(st.address, 0u);
	ASSERT_EQ(st.tokens.size(), 2u);
	ASSERT_EQ(st.tokens[0], 127u);
	ASSERT_EQ(st.tokens[1], 255u);
	ASSERT_EQ(st.capture_groups.size(), 1u);
	ASSERT_EQ(st.capture_groups.count("a"), 1u);
	ASSERT_EQ(st.capture_groups["a"], 255u);
	ASSERT_EQ(st.mnemonics.size(), 1u);
	ASSERT_EQ(st.mnemonics.front().opcode, std::string("1"));
	ASSERT_EQ(st.mnemonics.front().area, po::bound(0,1));
	ASSERT_TRUE(st.mnemonics.front().instructions.empty());
	ASSERT_EQ(st.jumps.size(), 0u);
}
*/
#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use layer::{Cell,OpaqueLayer};
    use guard::Guard;
    use value::Rvalue;

    #[test]
    fn decode_macro() {
        let lock_prfx = new_disassembler!(u8 =>
            [ 0x06 ] = |x| { true }
        );

        let main = new_disassembler!(u8 =>
            [ 22 , 21, lock_prfx ] = |x| { true },
            [ "....11 d@00"         ] = |x| true,
            [ "....11 d@00", ".. d@0011. 0" ] = |x| true
        );
    }

    fn fixture() -> (Rc<Disassembler<u8>>,Rc<Disassembler<u8>>,Rc<Disassembler<u8>>,OpaqueLayer) {
        let sub = new_disassembler!(u8 =>
            [ 2 ] = |st: &mut State<u8>| {
                let next = st.address;
                st.mnemonic(2,"BA","",vec!(),|_| {});
                st.jump(Rvalue::Constant(next + 2),Guard::new());
                true
            });
        let sub2 = new_disassembler!(u8 =>
            [ 8 ] = |_| false);

        let mut main = new_disassembler!(u8 =>
            [ 1, sub ] = |_| true,
            [ 1 ] = |st: &mut State<u8>| {
                let next = st.address;
                st.mnemonic(1,"A","",vec!(),|_| {});
                st.jump(Rvalue::Constant(next + 1),Guard::new());
                true
            },
            [ "0 k@..... 11" ] = |st: &mut State<u8>| {
                let next = st.address;
                st.mnemonic(1,"C","",vec!(),|_| {});
                st.jump(Rvalue::Constant(next + 1),Guard::new());
                true
            },
            _ = |st: &mut State<u8>| {
                let next = st.address;
                st.mnemonic(1,"UNK","",vec!(),|_| {});
                st.jump(Rvalue::Constant(next + 1),Guard::new());
                true
            }
		);

        (sub,sub2,main,OpaqueLayer::wrap(vec!(1,1,2,1,3,8,1,8)))
	}

    #[test]
    fn single_decoder()
    {
        let (_,_,main,def) = fixture();
        let mut st = State::<u8>::new(0);

        assert!(main.next_match(&mut def.iter(),st).is_some());
        /*
        ASSERT_EQ(i->first, ++bytes.begin());
        ASSERT_EQ(st.address, 0u);
        ASSERT_GE(st.tokens.size(), 1u);
        ASSERT_EQ(st.tokens[0], 'A');
        ASSERT_EQ(st.capture_groups.size(), 0u);
        ASSERT_EQ(st.mnemonics.size(), 1u);
        ASSERT_EQ(st.mnemonics.front().opcode, std::string("A"));
        ASSERT_EQ(st.mnemonics.front().area, po::bound(0,1));
        ASSERT_TRUE(st.mnemonics.front().instructions.empty());
        ASSERT_EQ(st.jumps.size(), 1u);
        ASSERT_TRUE(is_constant(st.jumps.front().first));
        ASSERT_EQ(to_constant(st.jumps.front().first).content(), 1u);
        ASSERT_TRUE(st.jumps.front().second.relations.empty());
        */
    }
}