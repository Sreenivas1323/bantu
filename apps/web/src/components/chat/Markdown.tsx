import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

/** Assistant text. Links open in a new tab; styling lives in `.md` in index.css. */
export function Markdown({ text }: { text: string }) {
  return (
    <div className="md">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ node: _node, ...props }) => <a {...props} target="_blank" rel="noreferrer" />,
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}
