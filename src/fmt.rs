
// @file template.rs
// @brief run-time template formatter (for filename formatting)

use anyhow::{anyhow, Context, Result};
use num_runtime_fmt::NumFmt;
use regex::Regex;
use std::collections::HashMap;

use crate::eval::{Rpn, VarAttr};

#[derive(Debug)]
struct TemplateElement {
    fixed: String,
    var: Option<(Rpn, NumFmt)>,
}

#[derive(Debug)]
struct Template {
    elems: Vec<TemplateElement>,
}

impl Template {
    pub fn from_str(input: &str, vars: Option<&HashMap<&[u8], VarAttr>>) -> Result<Self> {
        // parsing std::fmt-style formatter string
        // TODO: is there any good crate to do this? better avoid re-inventing wheels...
        let brace_matcher = Regex::new(r"\{.*?\}").unwrap();
        let arg_matcher = Regex::new(r"\{((?:\([^:]*\))|[a-zA-Z]*)(:{0,1}.*)\}").unwrap();

        let mut elems = Vec::new();
        let mut i = 0;

        for b in brace_matcher.captures_iter(input) {
            let b = b.get(0).unwrap();   // never fail
            let args = arg_matcher.captures(b.as_str()).with_context(|| anyhow!("unparsable format string: {:?}", b.as_str()))?;

            // args looks sane at the top level. then break {name:spec} into name and spec
            let name = args.get(1).unwrap();
            eprintln!("name: {:?}", name);

            let name = Rpn::new(name.as_str(), vars)?;

            let spec = args.get(2).unwrap();
            let spec = if spec.as_str().len() == 0 { "" } else { &spec.as_str()[1..] };
            let spec = NumFmt::from_str(spec)?;

            // expression (variable) and formatter specifier are both sane; move onto composing TemplateElement
            let elem = TemplateElement {
                fixed: input[i..b.start()].to_string(),
                var: Some((name, spec)),
            };
            elems.push(elem);

            i = b.end();
        }

        // the last element, which is a fixed-alone one
        let fixed = input[i..].to_string();
        elems.push(TemplateElement { fixed, var: None });

        Ok(Template { elems })
    }

    pub fn render<F>(&self, get: F) -> Result<String>
    where
        F: FnMut(usize, i64) -> i64,
    {
        let mut get = get;

        let mut s = String::new();
        for elem in &self.elems {
            s.push_str(&elem.fixed);

            if let Some(var) = &elem.var {
                let val = var.0.evaluate(&mut get)?;
                let fmt = var.1.fmt(val)?;
                s.push_str(&fmt);
            }
        }
        Ok(s)
    }
}

#[test]
fn test_template_new() {
    let vars = [
        (b"a", VarAttr { is_array: false, id: 0 }),
        (b"b", VarAttr { is_array: false, id: 1 }),
        (b"c", VarAttr { is_array: false, id: 2 }),
    ];
    let vars: HashMap<&[u8], VarAttr> = vars.iter().map(|(x, y)| (x.as_slice(), *y)).collect();

    assert!(Template::from_str("name", Some(&vars)).is_ok());
    assert!(Template::from_str("prefix{a}", Some(&vars)).is_ok());
    assert!(Template::from_str("{a}suffix", Some(&vars)).is_ok());
    assert!(Template::from_str("prefix{a}suffix", Some(&vars)).is_ok());

    assert!(Template::from_str("{a:}", Some(&vars)).is_ok());
    assert!(Template::from_str("{a:01d}", Some(&vars)).is_ok());
    assert!(Template::from_str("{a:01d}_{a:06x}_{a:#}", Some(&vars)).is_ok());
    assert!(Template::from_str("prefix_{a:-01d}_mid1_{c:01d}_mid2_{b:01d}_suffix", Some(&vars)).is_ok());

    assert!(Template::from_str("{:}", Some(&vars)).is_err());
    assert!(Template::from_str("{:x}", Some(&vars)).is_err());
    assert!(Template::from_str("{:?}", Some(&vars)).is_err());

    // FIXME: we want to make this an error
    assert!(Template::from_str("{:?", Some(&vars)).is_ok());

    // expressions
    assert!(Template::from_str("{(a + 1):}", Some(&vars)).is_ok());
    assert!(Template::from_str("{(2 * a + 1):}", Some(&vars)).is_ok());
    assert!(Template::from_str("{(a + a - 1):}", Some(&vars)).is_ok());
    assert!(Template::from_str("{(a & 0x01):}", Some(&vars)).is_ok());

    assert!(Template::from_str("{a + 1:}", Some(&vars)).is_err());
}

#[test]
fn test_template_render() {
    let vars = [
        (b"a", VarAttr { is_array: false, id: 0 }),
        (b"b", VarAttr { is_array: false, id: 1 }),
        (b"c", VarAttr { is_array: false, id: 2 }),
    ];
    let vars: HashMap<&[u8], VarAttr> = vars.iter().map(|(x, y)| (x.as_slice(), *y)).collect();

    macro_rules! test {
        ( $pattern: expr, $expected: expr ) => {
            let t = Template::from_str($pattern, Some(&vars)).unwrap();
            let rendered = t.render(|id, _| id as i64).unwrap();

            assert_eq!(rendered, $expected);
        };
    }

    // without variable
    test!("", "");
    test!("aaa", "aaa");

    // simple ones
    test!("{a}", "0");
    test!("{a:08x}", "00000000");
    test!("{a:02x}-{b:02x}-{c:02x}", "00-01-02");
    test!("prefix:{a:05d}:suffix", "prefix~00000~suffix");

    // expressions
    test!("{(2 * a + 1):}", "1");
    test!("{(2 * a + 1):} {(b | 0x02):} {(c ** c):}", "1 3 4");
}

// end of template.rs
