import { Footer, Layout, Navbar } from "nextra-theme-docs";
import { Banner, Head } from "nextra/components";
import { getPageMap } from "nextra/page-map";
import "nextra-theme-docs/style.css";

export const metadata = {
  title: "Labcoat Docs",
  description:
    "Documentation for Labcoat, a toolkit for smart contracts on Bitcoin",
};

const banner = (
  <Banner storageKey="labcoat-docs">
    📘 Welcome to Labcoat Docs — Version aligned with your repo
  </Banner>
);
const navbar = (
  <Navbar
    logo={<b>Labcoat</b>}
    projectLink="https://github.com/jonatns/labcoat"
  />
);
const footer = <Footer>© {new Date().getFullYear()} Labcoat.</Footer>;

export default async function RootLayout({ children }) {
  return (
    <html lang="en" dir="ltr" suppressHydrationWarning>
      <Head />
      <body>
        <Layout
          banner={banner}
          navbar={navbar}
          pageMap={await getPageMap()}
          docsRepositoryBase="https://github.com/shuding/nextra/tree/main/docs"
          footer={footer}
        >
          {children}
        </Layout>
      </body>
    </html>
  );
}
