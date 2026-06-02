import { Badge } from "@/components/ui/badge"

const styles: Record<string, string> = {
  database: "bg-purple-100 text-purple-800 hover:bg-purple-100",
  oauth2:   "bg-blue-100 text-blue-800 hover:bg-blue-100",
  saml:     "bg-orange-100 text-orange-800 hover:bg-orange-100",
}

export function StrategyBadge({ strategy }: { strategy: string }) {
  return <Badge className={styles[strategy] ?? "bg-gray-100 text-gray-800"}>{strategy}</Badge>
}
