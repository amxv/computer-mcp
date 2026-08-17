export const siteConfig = {
  name: "zodex",
  strapline: "ChatGPT-native coding on real machines",
  description:
    "Documentation for zodex: two first-class execution modes, Local on a trusted Apple Silicon Mac and Sprite on wake-on-demand remote Linux, sharing the same command, stdin, and patch MCP surface.",
  repoUrl: "https://github.com/amxv/zodex",
  accentColor: "#e5482d",
  accentColorDark: "#ff684d",
  footerSections: [
    {
      title: "zodex",
      text:
        "ChatGPT-native coding on real machines: trusted Local Mac execution or wake-on-demand Sprite Linux through the canonical Worker."
    },
    {
      title: "What this site covers",
      text:
        "Zodex Local, Sprite deployment, write modes, MCP tooling, observability, service operations, and the runtime behavior agents depend on."
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
  "Local",
  "Sprite",
  "Reference"
] as const;

export const primaryNav = [
  { href: "/docs", label: "Docs" },
  { href: siteConfig.repoUrl, label: "GitHub", external: true }
];
