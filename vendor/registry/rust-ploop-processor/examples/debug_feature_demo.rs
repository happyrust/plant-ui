//! 调试功能演示示例
//! 
//! 展示如何使用debug feature来控制调试信息的输出

use rust_ploop_processor::*;

fn main() -> Result<()> {
    println!("🧪 Rust PLOOP Processor 调试功能演示");
    println!("═══════════════════════════════════\n");

    // 测试调试宏
    debug_println!("这条消息只在启用debug feature时显示");
    debug_info!("这是一条带前缀的调试信息");

    println!("普通输出: 这条消息总是显示");

    // 创建简单的测试数据
    let test_data = r#"
NEW FRMWORK TEST_FRAMEWORK
NEW PLOOP
HEIG 100mm
NEW PAVERT
POS E 0mm N 0mm U 0mm
NEW PAVERT  
POS E 100mm N 0mm U 0mm
NEW PAVERT
POS E 100mm N 100mm U 0mm
NEW PAVERT
POS E 0mm N 100mm U 0mm
END
"#;

    println!("\n📝 测试PLOOP数据解析（带调试信息）:");
    
    // 创建解析器和处理器
    let parser = PLoopParser::new(1.0);
    let processor = PLoopProcessor::new();
    
    match parser.parse_file(test_data) {
        Ok(ploops) => {
            println!("✅ 解析成功! 发现 {} 个PLOOP", ploops.len());
            
            if let Some(ploop) = ploops.first() {
                println!("   PLOOP名称: {}", ploop.name);
                println!("   原始顶点数: {}", ploop.vertices.len());
                println!("   高度: {:.1}mm", ploop.height);
                
                // 处理PLOOP
                match processor.process_ploop(ploop) {
                    Ok(processed_vertices) => {
                        println!("   处理后顶点数: {}", processed_vertices.len());
                        
                        // 计算面积（简单矩形）
                        let area = 100.0 * 100.0; // 100mm x 100mm
                        println!("   面积: {:.2} mm²", area);
                    }
                    Err(e) => {
                        println!("   ❌ 处理失败: {}", e);
                    }
                }
            }
        }
        Err(e) => {
            println!("❌ 解析失败: {}", e);
        }
    }

    println!("\n💡 提示:");
    println!("   - 使用 'cargo run --example debug_feature_demo' 运行（无调试信息）");
    println!("   - 使用 'cargo run --example debug_feature_demo --features debug' 运行（有调试信息）");
    
    Ok(())
}
