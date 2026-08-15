export const siteConfig = {
  name: "zodex",
  strapline: "ChatGPT-native coding workspace",
  description:
    "Documentation for zodex, a ChatGPT-native coding workspace that gives GPT models a real Linux machine through Zodex Sprite or persistent Apple Silicon Zodex Local, the same command/stdin/patch MCP surface, and operator-chosen GitHub write modes.",
  repoUrl: "https://github.com/amxv/zodex",
  accentColor: "#e5482d",
  accentColorDark: "#ff684d",
  footerSections: [
    {
      title: "zodex",
      text:
        "A ChatGPT-native coding workspace for real Linux work on Sprite or Apple Silicon Local, normal Git workflows, and operator-controlled write autonomy."
    },
    {
      title: "What this site covers",
      text:
        "ChatGPT setup, Sprite deployment, Local isolation and lifecycle, write modes, GitHub permissions, MCP tooling, service operations, and the runtime behavior agents depend on."
    },
    {
      title: "Repository",
      linkPrefix: "Source: ",
      linkHref: "https://github.com/amxv/zodex",
      linkLabel: "github.com/amxv/zodex"
    }
  ]
} as const;

export const docCategories = [
  "Start",
  "Architecture",
  "GitHub Access",
  "Operations",
  "Reference"
] as const;

export const primaryNav = [
  { href: "/docs", label: "Docs" },
  { href: siteConfig.repoUrl, label: "GitHub", external: true }
];
