import { Badge } from "@/components/ui/badge"

const styles: Record<string, string> = {
  active:    "bg-green-100 text-green-800 hover:bg-green-100",
  read_only: "bg-amber-100 text-amber-800 hover:bg-amber-100",
  migrating: "bg-blue-100 text-blue-800 hover:bg-blue-100",
}

export function StatusBadge({ status }: { status: string }) {
  return <Badge className={styles[status] ?? "bg-gray-100 text-gray-800"}>{status}</Badge>
}
