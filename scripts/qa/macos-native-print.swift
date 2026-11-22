import AppKit
import PDFKit

let args=CommandLine.arguments
let mode=args[1]
if mode=="verify" {
    guard let doc=PDFDocument(url:URL(fileURLWithPath:args[2])),doc.pageCount>0 else {fatalError("Invalid delivered PDF")}
    guard let text=doc.string,text.contains(args[3]) else {fatalError("Delivered PDF is missing its printed content")}
    let size=doc.page(at:0)!.bounds(for:.mediaBox).size
    if args.count>5 {
        guard let w=Double(args[4]),let h=Double(args[5]),abs(size.width-w)<1,abs(size.height-h)<1 else {fatalError("Receipt paper dimensions were not preserved")}
    }
    print("Verified \(doc.pageCount) page(s), \(size.width)x\(size.height) points")
} else {
    class ReceiptView:NSView {
        var content=""
        override var isFlipped:Bool {true}
        override func draw(_ rect:NSRect) {
            NSColor.white.setFill();bounds.fill()
            (content as NSString).draw(in:NSRect(x:12,y:20,width:bounds.width-24,height:bounds.height-40),withAttributes:[.font:NSFont.systemFont(ofSize:12),.foregroundColor:NSColor.black])
        }
    }
    _=NSApplication.shared
    let view=ReceiptView(frame:NSRect(x:0,y:0,width:227,height:425))
    view.content=args[3]
    if mode=="fixture" {
        try view.dataWithPDF(inside:view.bounds).write(to:URL(fileURLWithPath:args[2]))
    } else {
        let info=NSPrintInfo()
        guard let printer=NSPrinter(name:args[2]) else {fatalError("Native printer not found")}
        info.printer=printer
        info.paperSize=NSSize(width:227,height:425)
        info.topMargin=0;info.bottomMargin=0;info.leftMargin=0;info.rightMargin=0
        info.horizontalPagination = .fit;info.verticalPagination = .fit
        let operation=NSPrintOperation(view:view,printInfo:info)
        operation.jobTitle="VPS native AppKit receipt"
        operation.showsPrintPanel=false;operation.showsProgressPanel=false
        guard operation.run() else {fatalError("AppKit printing failed")}
        print("Native AppKit print submitted")
    }
}
