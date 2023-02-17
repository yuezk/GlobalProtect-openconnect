type ParseResult = {
  text: (selector: string) => string;
  all: (selector: string) => ParseResult[];
  attr: (selector: string, attr?: string) => string;
};

export function parseXml(xml: string): ParseResult {
  const parser = new DOMParser();
  const doc = parser.parseFromString(xml, "text/xml");

  return buildParseResult(doc.documentElement);
}

function buildParseResult(el: Element): ParseResult {
  return {
    text(selector) {
      return el.querySelector(selector)?.textContent ?? '';
    },
    all(selector) {
      return [...el.querySelectorAll(selector)].map((node) => {
        return buildParseResult(node);
      });
    },
    attr(attr, selector) {
      if (selector) {
        return el.querySelector(selector)?.getAttribute(attr) ?? '';
      }
      return el.getAttribute(attr) ?? '';
    },
  };
}
