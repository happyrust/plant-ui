"use client"

import type React from "react"

import { useState, useMemo } from "react"
import { ChevronDown, ChevronRight, Search, File, Folder, FolderOpen } from "lucide-react"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
} from "@/components/ui/dropdown-menu"
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from "@/components/ui/dialog"
import { Label } from "@/components/ui/label"
import { Copy, Trash, Edit, Plus, Download, Share2 } from "lucide-react"

interface TreeNode {
  id: string
  name: string
  type: "folder" | "file"
  children?: TreeNode[]
  parent?: string // 添加parent属性
}

const sampleData: TreeNode[] = [
  {
    id: "1",
    name: "src",
    type: "folder",
    children: [
      {
        id: "2",
        name: "components",
        type: "folder",
        children: [
          { id: "3", name: "Button.tsx", type: "file" },
          { id: "4", name: "Input.tsx", type: "file" },
          { id: "5", name: "Modal.tsx", type: "file" },
        ],
      },
      {
        id: "6",
        name: "pages",
        type: "folder",
        children: [
          { id: "7", name: "Home.tsx", type: "file" },
          { id: "8", name: "About.tsx", type: "file" },
          { id: "9", name: "Contact.tsx", type: "file" },
        ],
      },
      {
        id: "10",
        name: "utils",
        type: "folder",
        children: [
          { id: "11", name: "helpers.ts", type: "file" },
          { id: "12", name: "constants.ts", type: "file" },
        ],
      },
    ],
  },
  {
    id: "13",
    name: "public",
    type: "folder",
    children: [
      {
        id: "14",
        name: "images",
        type: "folder",
        children: [
          { id: "15", name: "logo.png", type: "file" },
          { id: "16", name: "banner.jpg", type: "file" },
        ],
      },
      { id: "17", name: "favicon.ico", type: "file" },
    ],
  },
  {
    id: "18",
    name: "package.json",
    type: "file",
  },
  {
    id: "19",
    name: "README.md",
    type: "file",
  },
]

interface TreeItemProps {
  node: TreeNode
  searchTerm: string
  expandedNodes: Set<string>
  onToggle: (nodeId: string) => void
  level: number
  onContextMenu: (e: React.MouseEvent, node: TreeNode) => void
}

function TreeItem({ node, searchTerm, expandedNodes, onToggle, level, onContextMenu }: TreeItemProps) {
  const isExpanded = expandedNodes.has(node.id)
  const hasChildren = node.children && node.children.length > 0

  // 高亮搜索匹配的文本
  const highlightText = (text: string, search: string) => {
    if (!search) return text

    const regex = new RegExp(`(${search})`, "gi")
    const parts = text.split(regex)

    return parts.map((part, index) =>
      regex.test(part) ? (
        <span key={index} className="bg-yellow-200 dark:bg-yellow-800 px-1 rounded">
          {part}
        </span>
      ) : (
        part
      ),
    )
  }

  return (
    <div>
      <div
        className="flex items-center gap-2 py-1 px-2 hover:bg-muted/50 rounded cursor-pointer group"
        style={{ paddingLeft: `${level * 20 + 8}px` }}
        onClick={() => hasChildren && onToggle(node.id)}
        onContextMenu={(e) => onContextMenu(e, node)}
      >
        <div className="w-4 h-4 flex items-center justify-center">
          {hasChildren ? (
            isExpanded ? (
              <ChevronDown className="w-4 h-4 text-muted-foreground" />
            ) : (
              <ChevronRight className="w-4 h-4 text-muted-foreground" />
            )
          ) : null}
        </div>

        <div className="w-4 h-4 flex items-center justify-center">
          {node.type === "folder" ? (
            isExpanded ? (
              <FolderOpen className="w-4 h-4 text-blue-500" />
            ) : (
              <Folder className="w-4 h-4 text-blue-500" />
            )
          ) : (
            <File className="w-4 h-4 text-gray-500" />
          )}
        </div>

        <span className="text-sm select-none flex-1">{highlightText(node.name, searchTerm)}</span>
      </div>

      {hasChildren && isExpanded && (
        <div>
          {node.children?.map((child) => (
            <TreeItem
              key={child.id}
              node={child}
              searchTerm={searchTerm}
              expandedNodes={expandedNodes}
              onToggle={onToggle}
              level={level + 1}
              onContextMenu={onContextMenu}
            />
          ))}
        </div>
      )}
    </div>
  )
}

export default function Component() {
  const [searchTerm, setSearchTerm] = useState("")
  const [expandedNodes, setExpandedNodes] = useState<Set<string>>(new Set(["1", "2"]))

  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; node: TreeNode | null }>({
    x: 0,
    y: 0,
    node: null,
  })
  const [isDialogOpen, setIsDialogOpen] = useState(false)
  const [dialogMode, setDialogMode] = useState<"rename" | "newFile" | "newFolder">("rename")
  const [newName, setNewName] = useState("")

  const toggleNode = (nodeId: string) => {
    const newExpanded = new Set(expandedNodes)
    if (newExpanded.has(nodeId)) {
      newExpanded.delete(nodeId)
    } else {
      newExpanded.add(nodeId)
    }
    setExpandedNodes(newExpanded)
  }

  // 过滤树形数据
  const filterTree = (nodes: TreeNode[], term: string): TreeNode[] => {
    if (!term) return nodes

    const filtered: TreeNode[] = []

    for (const node of nodes) {
      const matchesSearch = node.name.toLowerCase().includes(term.toLowerCase())
      const filteredChildren = node.children ? filterTree(node.children, term) : []

      if (matchesSearch || filteredChildren.length > 0) {
        filtered.push({
          ...node,
          children: filteredChildren.length > 0 ? filteredChildren : node.children,
        })

        // 如果有匹配项，自动展开父节点
        if (filteredChildren.length > 0) {
          setExpandedNodes((prev) => new Set([...prev, node.id]))
        }
      }
    }

    return filtered
  }

  const filteredData = useMemo(() => {
    return filterTree(sampleData, searchTerm)
  }, [searchTerm])

  const expandAll = () => {
    const getAllNodeIds = (nodes: TreeNode[]): string[] => {
      const ids: string[] = []
      for (const node of nodes) {
        ids.push(node.id)
        if (node.children) {
          ids.push(...getAllNodeIds(node.children))
        }
      }
      return ids
    }
    setExpandedNodes(new Set(getAllNodeIds(sampleData)))
  }

  const collapseAll = () => {
    setExpandedNodes(new Set())
  }

  const clearSearch = () => {
    setSearchTerm("")
  }

  // 添加处理右键菜单的函数
  const handleContextMenu = (e: React.MouseEvent, node: TreeNode) => {
    e.preventDefault()
    e.stopPropagation()
    setContextMenu({ x: e.clientX, y: e.clientY, node })
  }

  // 添加关闭上下文菜单的函数
  const closeContextMenu = () => {
    setContextMenu({ x: 0, y: 0, node: null })
  }

  // 添加重命名函数
  const handleRename = () => {
    if (!contextMenu.node) return
    setNewName(contextMenu.node.name)
    setDialogMode("rename")
    setIsDialogOpen(true)
    closeContextMenu()
  }

  // 添加删除函数
  const handleDelete = () => {
    if (!contextMenu.node) return

    // 创建一个深拷贝函数来操作树数据
    const deleteNode = (nodes: TreeNode[], nodeId: string): TreeNode[] => {
      return nodes.filter((node) => {
        if (node.id === nodeId) return false
        if (node.children) {
          node.children = deleteNode(node.children, nodeId)
        }
        return true
      })
    }

    const newData = deleteNode([...sampleData], contextMenu.node.id)
    // 在实际应用中，这里应该调用一个更新数据的函数
    console.log("删除节点:", contextMenu.node.name)
    console.log("更新后的数据:", newData)

    closeContextMenu()
  }

  // 添加新建文件/文件夹函数
  const handleNew = (type: "file" | "folder") => {
    if (!contextMenu.node || contextMenu.node.type !== "folder") return

    setDialogMode(type === "file" ? "newFile" : "newFolder")
    setNewName("")
    setIsDialogOpen(true)
    closeContextMenu()
  }

  // 添加保存对话框内容的函数
  const handleSaveDialog = () => {
    if (!contextMenu.node || !newName.trim()) {
      setIsDialogOpen(false)
      return
    }

    if (dialogMode === "rename") {
      // 在实际应用中，这里应该调用一个更新节点名称的函数
      console.log(`重命名 ${contextMenu.node.name} 为 ${newName}`)
    } else {
      // 生成新的唯一ID
      const newId = `new-${Date.now()}`
      const newNode: TreeNode = {
        id: newId,
        name: newName,
        type: dialogMode === "newFile" ? "file" : "folder",
        parent: contextMenu.node.id,
        children: dialogMode === "newFolder" ? [] : undefined,
      }

      // 在实际应用中，这里应该调用一个添加新节点的函数
      console.log(`在 ${contextMenu.node.name} 下创建新${dialogMode === "newFile" ? "文件" : "文件夹"}: ${newName}`)
    }

    setIsDialogOpen(false)
  }

  return (
    <Card className="w-full max-w-md mx-auto">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Folder className="w-5 h-5" />
          模型树浏览器
        </CardTitle>
        <div className="space-y-3">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
            <Input
              placeholder="搜索文件或文件夹..."
              value={searchTerm}
              onChange={(e) => setSearchTerm(e.target.value)}
              className="pl-10"
            />
            {searchTerm && (
              <Button
                variant="ghost"
                size="sm"
                className="absolute right-1 top-1/2 transform -translate-y-1/2 h-7 w-7 p-0"
                onClick={clearSearch}
              >
                ×
              </Button>
            )}
          </div>
          <div className="flex gap-2">
            <Button variant="outline" size="sm" onClick={expandAll}>
              全部展开
            </Button>
            <Button variant="outline" size="sm" onClick={collapseAll}>
              全部折叠
            </Button>
          </div>
        </div>
      </CardHeader>
      <CardContent className="p-0">
        <div className="max-h-96 overflow-y-auto border-t">
          {filteredData.length > 0 ? (
            filteredData.map((node) => (
              <TreeItem
                key={node.id}
                node={node}
                searchTerm={searchTerm}
                expandedNodes={expandedNodes}
                onToggle={toggleNode}
                level={0}
                onContextMenu={handleContextMenu}
              />
            ))
          ) : (
            <div className="p-4 text-center text-muted-foreground">{searchTerm ? "未找到匹配项" : "暂无数据"}</div>
          )}
        </div>
      </CardContent>
      {contextMenu.node && (
        <div
          className="fixed z-50"
          style={{
            top: `${contextMenu.y}px`,
            left: `${contextMenu.x}px`,
          }}
        >
          <DropdownMenu open={!!contextMenu.node} onOpenChange={() => closeContextMenu()}>
            <DropdownMenuContent className="w-56">
              <DropdownMenuItem onClick={handleRename}>
                <Edit className="mr-2 h-4 w-4" />
                <span>重命名</span>
              </DropdownMenuItem>
              <DropdownMenuItem onClick={handleDelete}>
                <Trash className="mr-2 h-4 w-4" />
                <span>删除</span>
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              {contextMenu.node.type === "folder" && (
                <>
                  <DropdownMenuItem onClick={() => handleNew("file")}>
                    <Plus className="mr-2 h-4 w-4" />
                    <span>新建文件</span>
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => handleNew("folder")}>
                    <Folder className="mr-2 h-4 w-4" />
                    <span>新建文件夹</span>
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                </>
              )}
              <DropdownMenuItem>
                <Copy className="mr-2 h-4 w-4" />
                <span>复制</span>
              </DropdownMenuItem>
              {contextMenu.node.type === "file" && (
                <DropdownMenuItem>
                  <Download className="mr-2 h-4 w-4" />
                  <span>下载</span>
                </DropdownMenuItem>
              )}
              <DropdownMenuItem>
                <Share2 className="mr-2 h-4 w-4" />
                <span>分享</span>
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      )}

      <Dialog open={isDialogOpen} onOpenChange={setIsDialogOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>
              {dialogMode === "rename" ? "重命名" : dialogMode === "newFile" ? "新建文件" : "新建文件夹"}
            </DialogTitle>
          </DialogHeader>
          <div className="grid gap-4 py-4">
            <div className="grid grid-cols-4 items-center gap-4">
              <Label htmlFor="name" className="text-right">
                名称
              </Label>
              <Input
                id="name"
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                className="col-span-3"
                autoFocus
              />
            </div>
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setIsDialogOpen(false)}>
              取消
            </Button>
            <Button type="button" onClick={handleSaveDialog}>
              保存
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  )
}
