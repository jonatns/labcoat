/**
 * Wrap each Markdown table in a focusable, labelled scroll region.
 *
 * Wide reference tables scroll sideways on narrow screens. A scrolling
 * element must be reachable by keyboard (WCAG 2.1.1), and making the table
 * itself scroll (`display: block`) also strips its table semantics in some
 * browsers. The wrapper takes the scrolling and the focus instead, and is
 * named after the section the table belongs to.
 */
const text = (node) =>
  node.type === 'text' ? node.value : (node.children ?? []).map(text).join('');

export default function rehypeScrollTables() {
  return (tree) => {
    let section = 'Table';
    const visit = (parent) => {
      parent.children?.forEach((node, index) => {
        if (node.type !== 'element') return;
        if (/^h[1-4]$/.test(node.tagName)) {
          section = text(node).trim() || section;
          return;
        }
        if (node.tagName === 'table') {
          parent.children[index] = {
            type: 'element',
            tagName: 'div',
            properties: { className: ['table-scroll'], tabIndex: 0, role: 'region', ariaLabel: `${section} table` },
            children: [node],
          };
          return;
        }
        visit(node);
      });
    };
    visit(tree);
  };
}
