use anyhow::Result;

const COMMANDS: &[&str] = &[
    "watermark",
    "page-numbers",
    "flip",
    "crop",
    "repair",
    "image-to-pdf",
];

pub fn run(command: Option<String>) -> Result<()> {
    match command.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        None => {
            println!("usage: ypdf spec-help <command>");
            println!("commands: {}", COMMANDS.join(" "));
            println!("Run this only when simple flags are not enough, then pass --spec.");
            Ok(())
        }
        Some(name) => {
            let text = help_for(name).ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown command {name}. spec-help supports: {}",
                    COMMANDS.join(" ")
                )
            })?;
            print!("{text}");
            Ok(())
        }
    }
}

fn help_for(name: &str) -> Option<&'static str> {
    Some(match name {
        "watermark" => {
            "\
watermark
simple:  ypdf watermark FILE --text CONFIDENTIAL
complex: ypdf watermark FILE --spec '{...}' [--image stamp.png]

--spec is API watermarkSpec (camelCase).
kind            text | image
text            stamp text when kind=text
fontSize        number, default 48
color           #rrggbb
rotate          degrees
opacity         0–1
layer           above | below
placement.mode  single | tile
placement.horizontal  left | center | right
placement.vertical    top | center | bottom
placement.gapX / gapY  tile spacing

example:
{\"kind\":\"text\",\"text\":\"CONFIDENTIAL\",\"fontSize\":36,\"opacity\":0.28,\"layer\":\"above\",\"placement\":{\"mode\":\"tile\",\"horizontal\":\"center\",\"vertical\":\"center\",\"gapX\":160,\"gapY\":120}}
"
        }
        "page-numbers" => {
            "\
page-numbers
simple:  ypdf page-numbers FILE
         ypdf page-numbers FILE --style '${p}/${n}' --position footer-center --font-size 12
complex: ypdf page-numbers FILE --spec '{...}'

--style is the template. ${p}=current page, ${n}=total. {p}/{n} is accepted.
--position  footer-center | footer-left | footer-right | header-center | header-left | header-right

--spec is API pageNumberSpec. Use it for per-range or interval styles.
mode     range (default) | interval
ranges[]  { to, show, countInTotal, restartCount, style }
  to             last page of this range (1-based, strictly increasing)
  style.template ${p}/${n}
  style.horizontal left | center | right
  style.vertical   top | bottom
  style.fontSize   8–48
  style.color      #rrggbb
interval / intervalStyles[]  repeating styles when mode=interval

example (8-page file, first 3 unnumbered):
{\"mode\":\"range\",\"ranges\":[{\"to\":3,\"show\":false,\"style\":{\"template\":\"${p}/${n}\"}},{\"to\":8,\"style\":{\"template\":\"${p}/${n}\",\"horizontal\":\"center\",\"vertical\":\"bottom\"}}]}
"
        }
        "flip" => {
            "\
flip
simple:  ypdf flip FILE --rotate 90
         ypdf flip FILE --rotate 180 --pages 1,3-4
complex: ypdf flip FILE --spec '{...}'

--rotate  90 | 180 | 270
--pages   1-based ranges, like 1,3-4 (omit = all pages)

--spec is API flipSpec.
rotate   apply this angle to every page when pages is omitted
pages[]  { page, rotate }  page is 1-based; rotate 90 | 180 | 270

example (page 1 only):
{\"pages\":[{\"page\":1,\"rotate\":90}]}
"
        }
        "crop" => {
            "\
crop
simple:  ypdf crop FILE --inset 0.1
         ypdf crop FILE --top 0.12 --bottom 0.12
complex: ypdf crop FILE --spec '{...}'

Insets are 0–0.45 fractions of page size.
--inset sets top/right/bottom/left to the same value.

--spec is API cropSpec.
top/right/bottom/left  apply to every page when ranges is omitted
mode    range (default) | interval
ranges[]  { to, top, right, bottom, left }

example (pages 1–12):
{\"mode\":\"range\",\"ranges\":[{\"to\":12,\"top\":0.12,\"right\":0.12,\"bottom\":0.12,\"left\":0.12}]}
"
        }
        "repair" => {
            "\
repair
simple:  ypdf repair FILE
complex: ypdf repair FILE --spec '{...}'  (only to turn items off)

--spec is API repairSpec. Omit it to use all defaults (true).
removeJavascript
normalizePageBoxes
repairBrokenDests
removeAdditionalActions
syncMetadata
keepSubmitActions

example:
{\"removeJavascript\":true,\"normalizePageBoxes\":true,\"repairBrokenDests\":true,\"removeAdditionalActions\":true,\"syncMetadata\":true,\"keepSubmitActions\":false}
"
        }
        "image-to-pdf" => {
            "\
image-to-pdf
simple:  ypdf image-to-pdf A.png B.jpg --format a4
complex: ypdf image-to-pdf A.png B.jpg --spec '{...}'

--format  a4 | fit | letter
--dpi     rasterize hint

--spec is API imagePdfSpec.
pageSize     a4 | fit | letter
orientation  portrait | landscape
marginMm     0–40
rotate       0 | 90 | 180 | 270
pages[]      per-file overrides; length MUST equal the number of input files

example:
{\"pageSize\":\"a4\",\"orientation\":\"landscape\",\"marginMm\":10,\"rotate\":0}
"
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_commands_without_a_name() {
        run(None).expect("list");
    }

    #[test]
    fn page_numbers_help_stays_narrow() {
        let text = help_for("page-numbers").expect("help");
        assert!(text.contains("${p}/${n}"));
        assert!(text.contains("pageNumberSpec"));
        assert!(text.contains("--position"));
        assert!(!text.contains("flipSpec"));
        assert!(!text.contains("cropSpec"));
    }

    #[test]
    fn unknown_command_fails() {
        let err = run(Some("merge".into())).expect_err("merge has no spec");
        assert!(err.to_string().contains("unknown command"));
    }
}
