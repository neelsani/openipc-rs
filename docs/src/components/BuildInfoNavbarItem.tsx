import React from "react";
import useDocusaurusContext from "@docusaurus/useDocusaurusContext";
import clsx from "clsx";

type BuildInfo = {
  commit: string | null;
  shortCommit: string | null;
  tag: string | null;
  dirty: boolean;
  commitUrl: string;
};

type NavbarItemProps = {
  className?: string;
};

function buildTitle(info: BuildInfo): string {
  const parts = [];
  if (info.commit) {
    parts.push(`commit ${info.commit}`);
  }
  if (info.tag) {
    parts.push(`tag ${info.tag}`);
  }
  if (info.dirty) {
    parts.push("dirty working tree");
  }
  return parts.length > 0 ? parts.join(" · ") : "build metadata unavailable";
}

export default function BuildInfoNavbarItem({ className }: NavbarItemProps) {
  const { siteConfig } = useDocusaurusContext();
  const info = siteConfig.customFields.buildInfo as BuildInfo | undefined;
  if (!info?.shortCommit) {
    return null;
  }

  const label = info.dirty ? `${info.shortCommit}+` : info.shortCommit;

  return (
    <a
      className={clsx("navbar__item navbarBuildInfo", className)}
      href={info.commitUrl}
      target="_blank"
      rel="noreferrer"
      title={buildTitle(info)}
      aria-label={buildTitle(info)}
    >
      <svg
        className="navbarBuildInfo__icon"
        viewBox="0 0 24 24"
        aria-hidden="true"
      >
        <path d="M3 12h6" />
        <circle cx="12" cy="12" r="3" />
        <path d="M15 12h6" />
      </svg>
      <span>{label}</span>
      {info.tag && <span className="navbarBuildInfo__tag">{info.tag}</span>}
    </a>
  );
}
